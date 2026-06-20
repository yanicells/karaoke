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
    case audioUnitFailed(String, OSStatus)
    case microphonePermissionDenied

    package var errorDescription: String? {
        switch self {
        case .deviceSetFailed(let status):
            return "Failed to set audio device (OSStatus \(status))."
        case .engineStartFailed(let error):
            return "Audio engine failed to start: \(error.localizedDescription)"
        case .audioUnitFailed(let operation, let status):
            return "\(operation) failed (OSStatus \(status))."
        case .microphonePermissionDenied:
            return "Microphone access denied. Go to System Settings → Privacy & Security → Microphone and enable KaraokeMic."
        }
    }
}
