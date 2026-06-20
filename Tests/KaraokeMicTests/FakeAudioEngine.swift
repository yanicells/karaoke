import CoreAudio
@testable import KaraokeMicCore

@MainActor
final class FakeAudioEngine: AudioEngineProtocol {
    var startCallCount = 0
    var stopCallCount = 0
    var isRunning = false
    var lastGain: Float?
    var lastMuted: Bool?
    var lastInputDeviceID: AudioDeviceID?
    var lastOutputDeviceID: AudioDeviceID?
    var shouldThrowOnStart = false
    var shouldThrowOnSetInputDevice = false
    var shouldThrowOnSetOutputDevice = false
    var estimatedLatencyMs: Double = 8.0
    var onConfigurationChange: (@MainActor () -> Void)?

    func start() async throws {
        if shouldThrowOnStart { throw FakeEngineError.startFailed }
        startCallCount += 1
        isRunning = true
    }

    func stop() {
        stopCallCount += 1
        isRunning = false
    }

    func setInputDevice(_ deviceID: AudioDeviceID) throws {
        if shouldThrowOnSetInputDevice { throw FakeEngineError.deviceSetFailed }
        lastInputDeviceID = deviceID
    }

    func setOutputDevice(_ deviceID: AudioDeviceID) throws {
        if shouldThrowOnSetOutputDevice { throw FakeEngineError.deviceSetFailed }
        lastOutputDeviceID = deviceID
    }

    func setGain(_ gain: Float) { lastGain = gain }
    func setMuted(_ muted: Bool) { lastMuted = muted }

    enum FakeEngineError: Error { case startFailed, deviceSetFailed }
}
