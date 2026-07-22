import SwiftUI

/// Hoofd-view: fullscreen video met daaroverheen een HUD (stats linksboven)
/// en een control bar (Start/Stop onderaan).
struct ContentView: View {
    @EnvironmentObject var appState: AppState
    @StateObject private var engine = StreamEngine()

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()

            VideoView(displayLayer: engine.decoder.displayLayer)
                .ignoresSafeArea()

            VStack(spacing: 0) {
                HUD()
                    .frame(maxWidth: .infinity, alignment: .leading)
                Spacer()
                ControlBar(
                    isListening: appState.isListening,
                    onStart: { engine.start(port: 5000, appState: appState) },
                    onStop:  { engine.stop(appState: appState) }
                )
            }
            .padding(.horizontal, 16)
            .padding(.top, 16)
            .padding(.bottom, 24)
        }
    }
}

/// Stats-blok linksboven: status, fps, resolutie, bitrate, datavolume.
struct HUD: View {
    @EnvironmentObject var appState: AppState

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Circle()
                    .fill(appState.isListening ? Color.green : Color.red)
                    .frame(width: 10, height: 10)
                Text(appState.status)
                    .font(.system(.caption, design: .monospaced))
                    .bold()
                    .foregroundColor(.white)
                    .lineLimit(1)
            }

            if appState.isListening {
                Group {
                    Text("FPS: \(appState.fps)")
                    if appState.width > 0 {
                        Text("Resolutie: \(appState.width) × \(appState.height)")
                    }
                    Text(String(format: "Bitrate: %.1f Mbps", appState.bitrateMbps))
                    Text("Data: \(appState.bytesTotal / (1024 * 1024)) MB")
                }
                .font(.system(size: 11, weight: .regular, design: .monospaced))
                .foregroundColor(.white.opacity(0.8))
            }

            if let err = appState.errorMessage {
                Text(err)
                    .font(.system(size: 11, weight: .semibold, design: .monospaced))
                    .foregroundColor(.red)
                    .lineLimit(2)
            }
        }
        .padding(12)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 10))
    }
}

/// Start/Stop-knoppenbalk onderaan.
struct ControlBar: View {
    let isListening: Bool
    let onStart: () -> Void
    let onStop: () -> Void

    var body: some View {
        HStack(spacing: 16) {
            Button(action: onStart) {
                Label("Start", systemImage: "play.fill")
                    .bold()
                    .padding(.vertical, 12)
                    .frame(maxWidth: .infinity)
                    .background(isListening ? Color.gray.opacity(0.5) : Color.blue)
                    .foregroundColor(.white)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
            }
            .disabled(isListening)

            Button(action: onStop) {
                Label("Stop", systemImage: "stop.fill")
                    .bold()
                    .padding(.vertical, 12)
                    .frame(maxWidth: .infinity)
                    .background(!isListening ? Color.gray.opacity(0.5) : Color.red)
                    .foregroundColor(.white)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
            }
            .disabled(!isListening)
        }
        .padding(12)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 12))
    }
}
