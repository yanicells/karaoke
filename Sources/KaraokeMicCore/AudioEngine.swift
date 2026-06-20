import AVFoundation
import CoreAudio

@MainActor
package final class AudioEngine: AudioEngineProtocol {
    private let avEngine = AVAudioEngine()
    package var onConfigurationChange: (@MainActor () -> Void)?
    private var currentGain: Float = 1.0
    private var muted = false

    package var isRunning: Bool { avEngine.isRunning }

    package var estimatedLatencyMs: Double {
        (avEngine.inputNode.presentationLatency + avEngine.outputNode.presentationLatency) * 1000
    }

    package init() {
        NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange,
            object: avEngine,
            queue: .main
        ) { [weak self] _ in
            guard let self else { return }
            MainActor.assumeIsolated {
                self.onConfigurationChange?()
            }
        }
    }

    package func start() async throws {
        let granted = await requestMicrophonePermission()
        guard granted else { throw AudioEngineError.microphonePermissionDenied }

        // mic → mainMixerNode (already wired to outputNode by AVAudioEngine)
        let format = avEngine.inputNode.outputFormat(forBus: 0)
        avEngine.connect(avEngine.inputNode, to: avEngine.mainMixerNode, format: format)
        avEngine.mainMixerNode.outputVolume = muted ? 0 : currentGain

        setPreferredBufferSize()

        do {
            try avEngine.start()
        } catch {
            throw AudioEngineError.engineStartFailed(error)
        }
    }

    package func stop() {
        avEngine.stop()
        // Disconnect the mic input so the graph can be reconfigured on next start.
        avEngine.disconnectNodeInput(avEngine.mainMixerNode)
    }

    package func setInputDevice(_ deviceID: AudioDeviceID) throws {
        guard let unit = avEngine.inputNode.audioUnit else { return }
        var id = deviceID
        let status = AudioUnitSetProperty(
            unit,
            kAudioOutputUnitProperty_CurrentDevice,
            kAudioUnitScope_Global,
            0,
            &id,
            UInt32(MemoryLayout<AudioDeviceID>.size)
        )
        guard status == noErr else { throw AudioEngineError.deviceSetFailed(status) }
    }

    package func setOutputDevice(_ deviceID: AudioDeviceID) throws {
        guard let unit = avEngine.outputNode.audioUnit else { return }
        var id = deviceID
        let status = AudioUnitSetProperty(
            unit,
            kAudioOutputUnitProperty_CurrentDevice,
            kAudioUnitScope_Global,
            0,
            &id,
            UInt32(MemoryLayout<AudioDeviceID>.size)
        )
        guard status == noErr else { throw AudioEngineError.deviceSetFailed(status) }
    }

    package func setGain(_ gain: Float) {
        currentGain = gain
        if !muted { avEngine.mainMixerNode.outputVolume = gain }
    }

    package func setMuted(_ m: Bool) {
        muted = m
        avEngine.mainMixerNode.outputVolume = m ? 0 : currentGain
    }

    private func setPreferredBufferSize() {
        guard let unit = avEngine.inputNode.audioUnit else { return }
        var frames: UInt32 = 256  // ~5.8 ms @ 44.1 kHz
        AudioUnitSetProperty(
            unit,
            kAudioDevicePropertyBufferFrameSize,
            kAudioUnitScope_Global,
            0,
            &frames,
            UInt32(MemoryLayout<UInt32>.size)
        )
    }
}

private func requestMicrophonePermission() async -> Bool {
    switch AVCaptureDevice.authorizationStatus(for: .audio) {
    case .authorized: return true
    case .notDetermined: return await AVCaptureDevice.requestAccess(for: .audio)
    case .denied, .restricted: return false
    @unknown default: return false
    }
}
