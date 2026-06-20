import Observation
import CoreAudio

@MainActor
@Observable
package final class MicController {
    package var isRunning: Bool = false
    package var isMuted: Bool = false
    package var gain: Float = 1.0
    package var errorMessage: String?

    package init() {}
    package func toggleRunning() async {}
    package func toggleMute() {}
    package func refreshDevices() {}
}
