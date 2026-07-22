import CRustCore
import Foundation
import SwiftUI

/// Koppelvlak tussen de Rust core en de Swift UI.
///
/// `start()` roept `hyprpad_start()` aan met:
/// - één NALU-callback (`onNalu`) die op de Rust parser-thread binnenkomt
/// - één log-callback (`onLog`) voor status/foutmeldingen
/// - een `Unmanaged<StreamContext>` pointer als ctx
///
/// Stats (fps/bytes/resolutie) worden **niet** via callbacks gepusht — Swift
/// polt ze via `hyprpad_stats()` op een 0.25s Timer en werkt de main-actor
/// AppState bij. Dit was de voornaamste crash-oorzaak in de vorige app: de
/// callback liep op een Rust-thread en raakte SwiftUI-state aan zonder hop.
///
/// `StreamEngine` zelf is `@MainActor`-geïsoleerd: alleen de timer + lifecycle
/// methodes zitten erop. De C-callbacks zijn `static` en raken géén
/// instance-state (alleen de `Unmanaged<StreamContext>` pointer).
@MainActor
final class StreamEngine: ObservableObject {
    let decoder = H264Decoder()
    private var context: StreamContext?
    private var statsTimer: Timer?

    func start(port: UInt16, appState: AppState) {
        // Context wordt op de heap gehouden door Unmanaged.passRetained.
        let ctx = StreamContext(decoder: decoder)
        let ctxPtr = Unmanaged.passRetained(ctx).toOpaque()
        self.context = ctx

        let callbacks = HyprpadCallbacks(
            on_nalu: StreamEngine.onNalu,
            on_log: StreamEngine.onLog
        )

        let started = hyprpad_start(port, callbacks, ctxPtr)

        if started {
            appState.bitrateRefDate = Date()
            appState.bitrateRefBytes = 0
            startStatsTimer(appState: appState)
        } else {
            Unmanaged<StreamContext>.fromOpaque(ctxPtr).release()
            self.context = nil
            appState.isListening = false
            appState.status = "Kon niet starten (UDP bind?)"
        }
    }

    func stop(appState: AppState) {
        hyprpad_stop()
        statsTimer?.invalidate()
        statsTimer = nil

        if let ctx = context {
            let ptr = Unmanaged.passUnretained(ctx).toOpaque()
            Unmanaged<StreamContext>.fromOpaque(ptr).release()
        }
        context = nil
        decoder.reset()

        appState.isListening = false
        appState.status = "Gestopt"
        appState.fps = 0
        appState.bitrateMbps = 0
    }

    private func startStatsTimer(appState: AppState) {
        statsTimer = Timer.scheduledTimer(withTimeInterval: 0.25, repeats: true) { _ in
            let s = hyprpad_stats()
            applyStatsToAppState(s, appState)
        }
    }

    // MARK: - C-callbacks (@convention(c))
    //
    // Deze callbacks komen op een Rust-achtergrondthread binnen.
    // Ze zijn `static` — ze raken géén instance-state. Alles wat ze doen:
    // 1. pointer unwrappen
    // 2. bytes kopiëren
    // 3. dispatch naar de decoder-eigen serial queue (zie H264Decoder)
    // Géén SwiftUI, géén main-thread access vanuit deze callbacks.

    private static let onNalu: OnNalu = { dataPtr, len, nalType, ctxPtr in
        guard let dataPtr = dataPtr, len > 0, let ctxPtr = ctxPtr else { return }
        let ctx = Unmanaged<StreamContext>.fromOpaque(ctxPtr).takeUnretainedValue()

        // Kopieer de bytes — de Rust-pointer is enkel geldig tijdens deze call.
        let payload = Data(bytes: dataPtr, count: Int(len))

        // Dispatch naar de decoder-eigen serial queue (zie H264Decoder).
        ctx.decoder.dispatch(payload, nalType: nalType)
    }

    private static let onLog: OnLog = { level, msgPtr, ctxPtr in
        guard let msgPtr = msgPtr, let ctxPtr = ctxPtr else { return }
        let _ = Unmanaged<StreamContext>.fromOpaque(ctxPtr).takeUnretainedValue()
        let message = String(cString: msgPtr)

        let prefix = level == 0 ? "[hyprPad]" : (level == 1 ? "[hyprPad WARN]" : "[hyprPad ERR]")
        NSLog("\(prefix) \(message)")
    }
}

/// Heap-gealloceerde context die meegaat in de C-ABI pointer.
/// `@unchecked Sendable`: de decoder heeft zijn eigen serial queue; de
/// reference zelf is thread-safe zolang we niet tegelijk muteren.
private final class StreamContext: @unchecked Sendable {
    let decoder: H264Decoder
    init(decoder: H264Decoder) {
        self.decoder = decoder
    }
}
