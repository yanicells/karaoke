import Observation
import CoreAudio

@MainActor
@Observable
package final class MicController {
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
            try? engine.setInputDevice(d.id)
        }
    }

    package var selectedOutputDevice: AudioDevice? {
        didSet {
            guard let d = selectedOutputDevice else { return }
            try? engine.setOutputDevice(d.id)
        }
    }

    private var engine: any AudioEngineProtocol

    package init(engine: any AudioEngineProtocol = AudioEngine()) {
        self.engine = engine
        self.engine.onConfigurationChange = { [weak self] in
            self?.handleConfigurationChange()
        }
    }

    package func toggleRunning() async {
        if isRunning {
            engine.stop()
            isRunning = false
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
        errorMessage = "Audio device changed or disconnected. Re-select your devices and tap Start."
    }
}
