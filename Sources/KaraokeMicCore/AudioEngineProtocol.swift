import CoreAudio

@MainActor
package protocol AudioEngineProtocol: AnyObject {
    func start() async throws
    func stop()
    var isRunning: Bool { get }
    func setInputDevice(_ deviceID: AudioDeviceID) throws
    func setOutputDevice(_ deviceID: AudioDeviceID) throws
    func setGain(_ gain: Float)
    func setMuted(_ muted: Bool)
    var estimatedLatencyMs: Double { get }
    var onConfigurationChange: (@MainActor () -> Void)? { get set }
}

package enum AudioEngineError: Error, LocalizedError {
    case deviceSetFailed(OSStatus)
    case engineStartFailed(Error)
    case microphonePermissionDenied

    package var errorDescription: String? {
        switch self {
        case .deviceSetFailed(let status):
            return "Failed to set audio device (OSStatus \(status))."
        case .engineStartFailed(let error):
            return "Audio engine failed to start: \(error.localizedDescription)"
        case .microphonePermissionDenied:
            return "Microphone access denied. Go to System Settings → Privacy & Security → Microphone and enable KaraokeMic."
        }
    }
}

// Placeholder — deleted when AudioEngine.swift is created in Task 4.
@MainActor
package final class AudioEngine: AudioEngineProtocol {
    package var isRunning: Bool = false
    package var estimatedLatencyMs: Double = 0.0
    package var onConfigurationChange: (@MainActor () -> Void)?
    package init() {}
    package func start() async throws {}
    package func stop() {}
    package func setInputDevice(_ deviceID: AudioDeviceID) throws {}
    package func setOutputDevice(_ deviceID: AudioDeviceID) throws {}
    package func setGain(_ gain: Float) {}
    package func setMuted(_ muted: Bool) {}
}
