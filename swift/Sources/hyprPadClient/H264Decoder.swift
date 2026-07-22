import AVFoundation
import CoreMedia
import Foundation
import os

/// H.264 VideoToolbox-decoder met AVCC-output naar `AVSampleBufferDisplayLayer`.
///
/// **Fixes t.o.v. de crashende vorige versie:**
///
/// 1. Eén serial queue (`h264.decode`) voor alle decoderingsstappen. Geen
///    VideoToolbox-calls direct op de Rust callback-thread, geen main-thread.
/// 2. `formatDescription` wordt opgebouwd zodra zowel SPS als PPS binnen zijn.
///    Pas daarna worden IDR/P-frames aangenomen. De vorige code probeerde al
///    te decoderen met `formatDescription == nil` → layer ging in failed-state.
/// 3. `kCMSampleAttachmentKey_NotSync` wordt correct gezet (false voor IDR,
///    true voor P-frames). Ontbrak, kon de renderer in de war sturen.
/// 4. `sampleBufferRenderer.flush()` wordt vooraf gegaan als `.failed` — niet
///    na een mislukte enqueue.
/// 5. Alle state-aanpassingen via `OSAllocatedUnfairLock` (iPadOS 16+),
///    veel sneller dan NSLock en geen pomping-risico.
final class H264Decoder: @unchecked Sendable {
    let displayLayer = AVSampleBufferDisplayLayer()

    /// Voor HUD-foutmeldingen.
    @MainActor var lastErrorMessage: String?

    private let queue = DispatchQueue(label: "h264.decode", qos: .userInteractive)

    /// Eén lock beschermt alle mutable decoder-state.
    private let lock = OSAllocatedUnfairLock(initialState: DecoderState())

    private struct DecoderState {
        var sps: Data?
        var pps: Data?
        var formatDescription: CMVideoFormatDescription?
    }

    init() {
        displayLayer.videoGravity = .resizeAspect
    }

    /// Dispatch een complete NAL-unit naar de decoder-queue.
    /// Veilig om vanuit elke thread aan te roepen (gebruikt door `onNalu`).
    func dispatch(_ nalu: Data, nalType: UInt8) {
        queue.async { [weak self] in
            self?.handle(nalu, nalType: nalType)
        }
    }

    // MARK: - Intern (op `h264.decode` queue)

    private func handle(_ nalu: Data, nalType: UInt8) {
        switch nalType {
        case 7: // SPS
            lock.withLock { $0.sps = nalu }
            rebuildFormatDescription()
        case 8: // PPS
            lock.withLock { $0.pps = nalu }
            rebuildFormatDescription()
        case 5: // IDR
            enqueueFrame(nalu, isIDR: true)
        case 1: // non-IDR (P-frame)
            enqueueFrame(nalu, isIDR: false)
        default:
            break
        }
    }

    /// Herbouw de CMVideoFormatDescription zodra SPS én PPS aanwezig zijn.
    private func rebuildFormatDescription() {
        let result: (CMVideoFormatDescription?, Bool)? = lock.withLock { state in
            guard let sps = state.sps, let pps = state.pps else {
                return (nil, false)
            }

            var formatDesc: CMVideoFormatDescription?
            let status = sps.withUnsafeBytes { spsBuf -> OSStatus in
                pps.withUnsafeBytes { ppsBuf -> OSStatus in
                    guard let spsBase = spsBuf.baseAddress?
                            .assumingMemoryBound(to: UInt8.self),
                          let ppsBase = ppsBuf.baseAddress?
                            .assumingMemoryBound(to: UInt8.self)
                    else { return -1 }

                    let pointers: [UnsafePointer<UInt8>] = [spsBase, ppsBase]
                    let sizes: [Int] = [sps.count, pps.count]

                    return CMVideoFormatDescriptionCreateFromH264ParameterSets(
                        allocator: kCFAllocatorDefault,
                        parameterSetCount: 2,
                        parameterSetPointers: pointers,
                        parameterSetSizes: sizes,
                        nalUnitHeaderLength: 4,
                        formatDescriptionOut: &formatDesc
                    )
                }
            }

            if status == noErr {
                state.formatDescription = formatDesc
                return (formatDesc, true)
            }
            return (nil, false)
        }

        // Bij gewijzigde formatDescription: flush de renderer zodat hij de nieuwe
        // SPS/PPS accepteert (voorkomt .failed state na resolutie-switch).
        if let _ = result?.0, result?.1 == true {
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                let r = self.displayLayer.sampleBufferRenderer
                if r.status == .failed {
                    r.flush()
                }
            }
        }
    }

    private func enqueueFrame(_ naluPayload: Data, isIDR: Bool) {
        // 1. Haal de huidige formatDescription op onder lock.
        let fmt: CMVideoFormatDescription? = lock.withLock { $0.formatDescription }
        guard let formatDescription = fmt else {
            // Geen SPS/PPS nog — wacht rustig; geen crash-nee drop-nee.
            // We flushen niets; bij de volgende IDR met SPS+PPS herstelt het.
            return
        }

        // 2. Bouw AVCC-payload: 4-byte big-endian length prefix + NAL bytes.
        var avcc = Data(count: 4 + naluPayload.count)
        var lengthBE = UInt32(naluPayload.count).bigEndian
        avcc.replaceSubrange(0..<4, with: Swift.withUnsafeBytes(of: &lengthBE) { Data($0) })
        avcc.replaceSubrange(4..<(4 + naluPayload.count), with: naluPayload)

        // 3. CMBlockBuffer met malloc (CoreMedia beheert de free via
        //    kCFAllocatorMalloc).
        let bufferLen = avcc.count
        var blockBuffer: CMBlockBuffer?

        let status = avcc.withUnsafeBytes { rawBuf -> OSStatus in
            guard let base = rawBuf.baseAddress else { return -1 }
            let mem = malloc(bufferLen)!
            memcpy(mem, base, bufferLen)

            return CMBlockBufferCreateWithMemoryBlock(
                allocator: kCFAllocatorDefault,
                memoryBlock: mem,
                blockLength: bufferLen,
                blockAllocator: kCFAllocatorMalloc,
                customBlockSource: nil,
                offsetToData: 0,
                dataLength: bufferLen,
                flags: 0,
                blockBufferOut: &blockBuffer
            )
        }

        guard status == kCMBlockBufferNoErr, let bBuf = blockBuffer else { return }

        // 4. SampleBuffer.
        var sampleBuffer: CMSampleBuffer?
        var sampleSize = bufferLen
        let sampleStatus = CMSampleBufferCreateReady(
            allocator: kCFAllocatorDefault,
            dataBuffer: bBuf,
            formatDescription: formatDescription,
            sampleCount: 1,
            sampleTimingEntryCount: 0,
            sampleTimingArray: nil,
            sampleSizeEntryCount: 1,
            sampleSizeArray: &sampleSize,
            sampleBufferOut: &sampleBuffer
        )

        guard sampleStatus == noErr, let sBuf = sampleBuffer else { return }

        // 5. Attachments: DisplayImmediately + NotSync correct zetten.
        if let array = CMSampleBufferGetSampleAttachmentsArray(sBuf, createIfNecessary: true) {
            let ns = array as NSArray
            if let dict = ns.firstObject as? NSMutableDictionary {
                dict[kCMSampleAttachmentKey_DisplayImmediately as NSString] = true
                dict[kCMSampleAttachmentKey_NotSync as NSString] = !isIDR
            }
        }

        // 6. Enqueue op main (de layer eist main-thread access).
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            let r = self.displayLayer.sampleBufferRenderer
            if r.status == .failed {
                r.flush()
            }
            r.enqueue(sBuf)
        }
    }

    func reset() {
        DispatchQueue.main.async { [weak self] in
            self?.displayLayer.sampleBufferRenderer.flush()
        }
        lock.withLock { state in
            state.formatDescription = nil
            state.sps = nil
            state.pps = nil
        }
    }
}
