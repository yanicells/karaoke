import Observation
import CoreAudio

@MainActor
@Observable
public final class MicController {
    package private(set) var isRunning: Bool = false
    package var isMuted: Bool = false
    package private(set) var estimatedLatencyMs: Double = 0
    package private(set) var errorMessage: String?

    package var gain: Float = 1.0 {
        didSet { engine.setGain(gain) }
    }

    package private(set) var inputDevices: [AudioDevice] = []
    package private(set) var outputDevices: [AudioDevice] = []

    package var selectedInputDevice: AudioDevice? {
        didSet {
            guard let d = selectedInputDevice else { return }
            do {
                try engine.setInputDevice(d.id)
                errorMessage = nil
                estimatedLatencyMs = engine.isRunning ? engine.estimatedLatencyMs : 0
            } catch {
                errorMessage = error.localizedDescription
                isRunning = engine.isRunning
                estimatedLatencyMs = isRunning ? engine.estimatedLatencyMs : 0
            }
        }
    }

    package var selectedOutputDevice: AudioDevice? {
        didSet {
            guard let d = selectedOutputDevice else { return }
            do {
                try engine.setOutputDevice(d.id)
                errorMessage = nil
                estimatedLatencyMs = engine.isRunning ? engine.estimatedLatencyMs : 0
            } catch {
                errorMessage = error.localizedDescription
                isRunning = engine.isRunning
                estimatedLatencyMs = isRunning ? engine.estimatedLatencyMs : 0
            }
        }
    }

    private var engine: any AudioEngineProtocol

    package init(engine: any AudioEngineProtocol = AudioEngine()) {
        self.engine = engine
        self.engine.onConfigurationChange = { [weak self] in
            self?.handleConfigurationChange()
        }
    }

    /// Public entry point for app targets outside this module (e.g. the Xcode
    /// app). Uses the real `AudioEngine`; tests use the `package init(engine:)`.
    public convenience init() {
        self.init(engine: AudioEngine())
    }

    package func toggleRunning() async {
        if isRunning {
            engine.stop()
            isRunning = false
            estimatedLatencyMs = 0
        } else {
            do {
                try await engine.start()
                isRunning = true
                errorMessage = nil
                estimatedLatencyMs = engine.estimatedLatencyMs
            } catch {
                errorMessage = error.localizedDescription
                isRunning = false
            }
        }
    }

    package func toggleMute() {
        isMuted.toggle()
        engine.setMuted(isMuted)
    }

    package func refreshDevices() {
        inputDevices = listInputDevices()
        outputDevices = listOutputDevices()
        if selectedInputDevice == nil || !inputDevices.contains(selectedInputDevice!) {
            selectedInputDevice = inputDevices.first
        }
        if selectedOutputDevice == nil || !outputDevices.contains(selectedOutputDevice!) {
            selectedOutputDevice = outputDevices.first
        }
    }

    private func handleConfigurationChange() {
        engine.stop()
        isRunning = false
        estimatedLatencyMs = 0
        errorMessage = "Audio device changed or disconnected. Re-select your devices and tap Start."
    }
}
