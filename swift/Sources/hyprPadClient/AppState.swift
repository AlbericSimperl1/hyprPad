import CRustCore
import Foundation

/// Gedeelde UI-state, bijgewerkt vanaf de main thread.
///
/// De Rust core verzamelt stats in een atomic struct en Swift polt deze via
/// `hyprpad_stats()` in een Timer — géén callbacks naar SwiftUI vanaf
/// achtergrondthreads (de crash-oorzaak in de vorige app).
@MainActor
final class AppState: ObservableObject {
    @Published var isListening: Bool = false
    @Published var status: String = "Gereed"
    @Published var fps: Int = 0
    @Published var width: Int = 0
    @Published var height: Int = 0
    @Published var bitrateMbps: Double = 0
    @Published var bytesTotal: Int64 = 0
    @Published var errorMessage: String?

    /// Interne boekhouding voor bitrate-berekening (niet @Published).
    /// `internal` zodat `StreamEngine` ze kan resetten bij start.
    var bitrateRefDate: Date = Date()
    var bitrateRefBytes: Int64 = 0

    /// Bijgewerkt vanuit de stats-poll timer.
    func applyStats(_ s: HyprpadStats) {
        // FPS
        fps = Int(s.fps)

        // Resolutie
        width = Int(s.width)
        height = Int(s.height)

        // Bytes + bitrate (delta over verstreken tijd).
        bytesTotal = Int64(s.bytes_total)
        let now = Date()
        let elapsed = now.timeIntervalSince(bitrateRefDate)
        if elapsed > 0.4 { // herzetsnelheid niet te nervieus
            let delta = bytesTotal - bitrateRefBytes
            bitrateMbps = (Double(delta) * 8) / (elapsed * 1_000_000)
            bitrateRefDate = now
            bitrateRefBytes = bytesTotal
        }

        // State mapping.
        switch s.state {
        case 0:
            isListening = false
            status = "Gestopt"
        case 1:
            isListening = true
            status = "Luisteren op poort 5000…"
        case 2:
            isListening = true
            status = "Stream actief"
        case 3:
            isListening = false
            status = "Fout — zie log"
        default:
            break
        }
    }
}

/// Brug tussen de C-ABI stats en AppState, voor gebruik door `StreamEngine`
/// (deze func mag ook vanuit een nonisolated context worden aangeroepen — hij
/// hopt zelf naar de main actor).
func applyStatsToAppState(_ stats: HyprpadStats, _ appState: AppState) {
    Task { @MainActor in
        appState.applyStats(stats)
    }
}
