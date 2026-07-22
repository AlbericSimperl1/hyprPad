import AVFoundation
import SwiftUI

/// SwiftUI-wrapper rond `AVSampleBufferDisplayLayer` — hardware-accelerated
/// video output zonder tussenkomende kopieën.
struct VideoView: UIViewRepresentable {
    let displayLayer: AVSampleBufferDisplayLayer

    func makeUIView(context: Context) -> SampleBufferVideoView {
        SampleBufferVideoView(displayLayer: displayLayer)
    }

    func updateUIView(_ uiView: SampleBufferVideoView, context: Context) {
        uiView.refreshFrame()
    }
}

/// UIView die de displayLayer edge-to-edge host. Zwarte achtergrond voor
/// letterboxing bij aspect-ratio verschillen.
final class SampleBufferVideoView: UIView {
    private let displayLayer: AVSampleBufferDisplayLayer

    init(displayLayer: AVSampleBufferDisplayLayer) {
        self.displayLayer = displayLayer
        super.init(frame: .zero)
        displayLayer.videoGravity = .resizeAspect
        layer.addSublayer(displayLayer)
        backgroundColor = .black
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        displayLayer.frame = bounds
    }

    func refreshFrame() {
        displayLayer.frame = bounds
    }
}
