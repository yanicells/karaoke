import Testing
@testable import KaraokeMicCore

@MainActor
struct MicControllerTests {
    @Test func start_callsEngineStartAndSetsRunning() async throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        await controller.toggleRunning()
        #expect(fake.startCallCount == 1)
        #expect(controller.isRunning == true)
        #expect(controller.errorMessage == nil)
    }

    @Test func stop_callsEngineStopAndClearsRunning() async throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        await controller.toggleRunning()  // start
        #expect(controller.isRunning == true)
        #expect(fake.startCallCount == 1)
        await controller.toggleRunning()  // stop
        #expect(fake.stopCallCount == 1)
        #expect(controller.isRunning == false)
    }

    @Test func startFailure_setsErrorMessageAndDoesNotRun() async throws {
        let fake = FakeAudioEngine()
        fake.shouldThrowOnStart = true
        let controller = MicController(engine: fake)
        await controller.toggleRunning()
        #expect(controller.isRunning == false)
        #expect(controller.errorMessage != nil)
    }

    @Test func mute_callsSetMutedTrueAndTogglesMuted() throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        controller.toggleMute()
        #expect(fake.lastMuted == true)
        #expect(controller.isMuted == true)
        controller.toggleMute()
        #expect(fake.lastMuted == false)
        #expect(controller.isMuted == false)
    }

    @Test func gainChange_callsSetGain() throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        #expect(fake.lastGain == nil)
        controller.gain = 1.5
        #expect(fake.lastGain == 1.5)
    }

    @Test func configurationChange_stopsRunningAndSetsError() async throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        await controller.toggleRunning()  // start
        fake.onConfigurationChange?()
        #expect(fake.stopCallCount == 1)
        #expect(controller.isRunning == false)
        #expect(controller.errorMessage != nil)
    }

    @Test func selectedInputDevice_callsEngineSetInputDevice() throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        let device = AudioDevice(id: 42, name: "Test Mic", isBluetooth: false)
        controller.selectedInputDevice = device
        #expect(fake.lastInputDeviceID == 42)
    }

    @Test func selectedOutputDevice_callsEngineSetOutputDevice() throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        let device = AudioDevice(id: 99, name: "Test Speaker", isBluetooth: false)
        controller.selectedOutputDevice = device
        #expect(fake.lastOutputDeviceID == 99)
    }

    @Test func refreshDevices_doesNotCrashAndYieldsArrays() throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        controller.refreshDevices()
        // Hardware may be absent in CI; just verify no crash and the properties are typed correctly.
        _ = controller.inputDevices as [AudioDevice]
        _ = controller.outputDevices as [AudioDevice]
    }
}
