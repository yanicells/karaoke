import AVFoundation
import AudioToolbox
import CoreAudio
import Foundation

@MainActor
package final class AudioEngine: AudioEngineProtocol {
    package var onConfigurationChange: (@MainActor () -> Void)?

    private var playthrough: HALPlaythrough?
    private var currentGain: Float = 1.0
    private var muted = false
    private var inputDeviceID: AudioDeviceID?
    private var outputDeviceID: AudioDeviceID?

    package var isRunning: Bool { playthrough?.isRunning == true }
    package var estimatedLatencyMs: Double { playthrough?.estimatedLatencyMs ?? 0 }

    package init() {}

    package func start() async throws {
        let granted = await requestMicrophonePermission()
        guard granted else { throw AudioEngineError.microphonePermissionDenied }

        let input = try inputDeviceID ?? defaultInputDeviceID()
        let output = try outputDeviceID ?? defaultOutputDeviceID()
        stop()

        let next = try HALPlaythrough(
            inputDeviceID: input,
            outputDeviceID: output,
            gain: muted ? 0 : currentGain
        )
        try next.start()
        playthrough = next
    }

    package func stop() {
        playthrough?.stop()
        playthrough = nil
    }

    package func setInputDevice(_ deviceID: AudioDeviceID) throws {
        inputDeviceID = deviceID
        if isRunning { try restartRunningRoute() }
    }

    package func setOutputDevice(_ deviceID: AudioDeviceID) throws {
        outputDeviceID = deviceID
        if isRunning { try restartRunningRoute() }
    }

    package func setGain(_ gain: Float) {
        currentGain = gain
        playthrough?.gain = muted ? 0 : gain
    }

    package func setMuted(_ m: Bool) {
        muted = m
        playthrough?.gain = m ? 0 : currentGain
    }

    private func restartRunningRoute() throws {
        let input = try inputDeviceID ?? defaultInputDeviceID()
        let output = try outputDeviceID ?? defaultOutputDeviceID()
        stop()

        let next = try HALPlaythrough(
            inputDeviceID: input,
            outputDeviceID: output,
            gain: muted ? 0 : currentGain
        )
        try next.start()
        playthrough = next
    }
}

private final class HALPlaythrough {
    private enum Constants {
        static let scratchFrames = 16_384
        static let fallbackSampleRate = 48_000.0
    }

    private let state: HALPlaythroughState
    private var inputUnit: AudioUnit?
    private var outputUnit: AudioUnit?
    private(set) var isRunning = false
    let estimatedLatencyMs: Double

    var gain: Float {
        get { state.gainPointer.pointee }
        set { state.gainPointer.pointee = newValue }
    }

    init(inputDeviceID: AudioDeviceID, outputDeviceID: AudioDeviceID, gain: Float) throws {
        let sampleRate =
            nominalSampleRate(outputDeviceID)
            ?? nominalSampleRate(inputDeviceID)
            ?? Constants.fallbackSampleRate

        let ringFrames = max(Int(sampleRate.rounded(.up)) * 2, Constants.scratchFrames * 2)
        self.state = HALPlaythroughState(
            sampleRate: sampleRate,
            ringFrames: ringFrames,
            scratchFrames: Constants.scratchFrames,
            gain: gain
        )

        let inputBufferFrames = deviceBufferFrameSize(inputDeviceID) ?? 256
        let outputBufferFrames = deviceBufferFrameSize(outputDeviceID) ?? 256
        self.estimatedLatencyMs =
            Double(inputBufferFrames + outputBufferFrames) / sampleRate * 1000.0

        do {
            self.inputUnit = try Self.makeHALUnit()
            self.outputUnit = try Self.makeHALUnit()
            try configureInputUnit(deviceID: inputDeviceID)
            try configureOutputUnit(deviceID: outputDeviceID)
        } catch {
            stop()
            throw error
        }
    }

    deinit {
        stop()
    }

    func start() throws {
        guard !isRunning else { return }
        guard let inputUnit, let outputUnit else {
            throw AudioEngineError.audioUnitFailed("Audio unit was not configured", -1)
        }

        try check(AudioUnitInitialize(inputUnit), "Initialize input audio unit")
        try check(AudioUnitInitialize(outputUnit), "Initialize output audio unit")
        try check(AudioOutputUnitStart(inputUnit), "Start input audio unit")
        do {
            try check(AudioOutputUnitStart(outputUnit), "Start output audio unit")
            isRunning = true
        } catch {
            AudioOutputUnitStop(inputUnit)
            throw error
        }
    }

    func stop() {
        if let inputUnit {
            AudioOutputUnitStop(inputUnit)
            AudioUnitUninitialize(inputUnit)
        }
        if let outputUnit {
            AudioOutputUnitStop(outputUnit)
            AudioUnitUninitialize(outputUnit)
        }
        if let inputUnit {
            AudioComponentInstanceDispose(inputUnit)
            self.inputUnit = nil
            state.inputUnit = nil
        }
        if let outputUnit {
            AudioComponentInstanceDispose(outputUnit)
            self.outputUnit = nil
        }
        isRunning = false
    }

    private func configureInputUnit(deviceID: AudioDeviceID) throws {
        guard let inputUnit else {
            throw AudioEngineError.audioUnitFailed("Create input audio unit", -1)
        }

        var enableInput: UInt32 = 1
        try check(
            AudioUnitSetProperty(
                inputUnit,
                kAudioOutputUnitProperty_EnableIO,
                kAudioUnitScope_Input,
                1,
                &enableInput,
                UInt32(MemoryLayout<UInt32>.size)
            ),
            "Enable input bus"
        )

        var disableOutput: UInt32 = 0
        try check(
            AudioUnitSetProperty(
                inputUnit,
                kAudioOutputUnitProperty_EnableIO,
                kAudioUnitScope_Output,
                0,
                &disableOutput,
                UInt32(MemoryLayout<UInt32>.size)
            ),
            "Disable input unit output bus"
        )

        var deviceID = deviceID
        try check(
            AudioUnitSetProperty(
                inputUnit,
                kAudioOutputUnitProperty_CurrentDevice,
                kAudioUnitScope_Global,
                0,
                &deviceID,
                UInt32(MemoryLayout<AudioDeviceID>.size)
            ),
            "Set input device"
        )

        var format = monoFloatFormat(sampleRate: state.sampleRate)
        try check(
            AudioUnitSetProperty(
                inputUnit,
                kAudioUnitProperty_StreamFormat,
                kAudioUnitScope_Output,
                1,
                &format,
                UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
            ),
            "Set input callback format"
        )

        var callback = AURenderCallbackStruct(
            inputProc: inputRenderCallback,
            inputProcRefCon: Unmanaged.passUnretained(state).toOpaque()
        )
        try check(
            AudioUnitSetProperty(
                inputUnit,
                kAudioOutputUnitProperty_SetInputCallback,
                kAudioUnitScope_Global,
                0,
                &callback,
                UInt32(MemoryLayout<AURenderCallbackStruct>.size)
            ),
            "Install input callback"
        )

        state.inputUnit = inputUnit
    }

    private func configureOutputUnit(deviceID: AudioDeviceID) throws {
        guard let outputUnit else {
            throw AudioEngineError.audioUnitFailed("Create output audio unit", -1)
        }

        var enableOutput: UInt32 = 1
        try check(
            AudioUnitSetProperty(
                outputUnit,
                kAudioOutputUnitProperty_EnableIO,
                kAudioUnitScope_Output,
                0,
                &enableOutput,
                UInt32(MemoryLayout<UInt32>.size)
            ),
            "Enable output bus"
        )

        var disableInput: UInt32 = 0
        try check(
            AudioUnitSetProperty(
                outputUnit,
                kAudioOutputUnitProperty_EnableIO,
                kAudioUnitScope_Input,
                1,
                &disableInput,
                UInt32(MemoryLayout<UInt32>.size)
            ),
            "Disable output unit input bus"
        )

        var deviceID = deviceID
        try check(
            AudioUnitSetProperty(
                outputUnit,
                kAudioOutputUnitProperty_CurrentDevice,
                kAudioUnitScope_Global,
                0,
                &deviceID,
                UInt32(MemoryLayout<AudioDeviceID>.size)
            ),
            "Set output device"
        )

        let outputChannels = try Self.outputChannelCount(for: outputUnit)
        var format = nonInterleavedFloatFormat(
            sampleRate: state.sampleRate,
            channels: UInt32(outputChannels)
        )
        try check(
            AudioUnitSetProperty(
                outputUnit,
                kAudioUnitProperty_StreamFormat,
                kAudioUnitScope_Input,
                0,
                &format,
                UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
            ),
            "Set output callback format"
        )

        var callback = AURenderCallbackStruct(
            inputProc: outputRenderCallback,
            inputProcRefCon: Unmanaged.passUnretained(state).toOpaque()
        )
        try check(
            AudioUnitSetProperty(
                outputUnit,
                kAudioUnitProperty_SetRenderCallback,
                kAudioUnitScope_Input,
                0,
                &callback,
                UInt32(MemoryLayout<AURenderCallbackStruct>.size)
            ),
            "Install output callback"
        )
    }

    private static func makeHALUnit() throws -> AudioUnit {
        var description = AudioComponentDescription(
            componentType: kAudioUnitType_Output,
            componentSubType: kAudioUnitSubType_HALOutput,
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0,
            componentFlagsMask: 0
        )
        guard let component = AudioComponentFindNext(nil, &description) else {
            throw AudioEngineError.audioUnitFailed("Find HAL output audio unit", -1)
        }

        var unit: AudioUnit?
        try check(AudioComponentInstanceNew(component, &unit), "Create HAL output audio unit")
        guard let unit else {
            throw AudioEngineError.audioUnitFailed("Create HAL output audio unit", -1)
        }
        return unit
    }

    private static func outputChannelCount(for unit: AudioUnit) throws -> Int {
        var format = AudioStreamBasicDescription()
        var size = UInt32(MemoryLayout<AudioStreamBasicDescription>.size)
        try check(
            AudioUnitGetProperty(
                unit,
                kAudioUnitProperty_StreamFormat,
                kAudioUnitScope_Output,
                0,
                &format,
                &size
            ),
            "Read output device format"
        )
        return max(Int(format.mChannelsPerFrame), 1)
    }
}

private final class HALPlaythroughState {
    let sampleRate: Double
    let ring: MonoRingBuffer
    let scratchFrames: Int
    let inputScratch: UnsafeMutablePointer<Float>
    let outputScratch: UnsafeMutablePointer<Float>
    let inputBufferList: UnsafeMutablePointer<AudioBufferList>
    let gainPointer: UnsafeMutablePointer<Float>
    var inputUnit: AudioUnit?

    init(sampleRate: Double, ringFrames: Int, scratchFrames: Int, gain: Float) {
        self.sampleRate = sampleRate
        self.ring = MonoRingBuffer(capacity: ringFrames)
        self.scratchFrames = scratchFrames

        self.inputScratch = UnsafeMutablePointer<Float>.allocate(capacity: scratchFrames)
        self.inputScratch.initialize(repeating: 0, count: scratchFrames)
        self.outputScratch = UnsafeMutablePointer<Float>.allocate(capacity: scratchFrames)
        self.outputScratch.initialize(repeating: 0, count: scratchFrames)
        self.gainPointer = UnsafeMutablePointer<Float>.allocate(capacity: 1)
        self.gainPointer.initialize(to: gain)

        self.inputBufferList = UnsafeMutablePointer<AudioBufferList>.allocate(capacity: 1)
        self.inputBufferList.initialize(
            to: AudioBufferList(
                mNumberBuffers: 1,
                mBuffers: AudioBuffer(
                    mNumberChannels: 1,
                    mDataByteSize: UInt32(scratchFrames * MemoryLayout<Float>.size),
                    mData: inputScratch
                )
            )
        )
    }

    deinit {
        inputBufferList.deinitialize(count: 1)
        inputBufferList.deallocate()
        inputScratch.deinitialize(count: scratchFrames)
        inputScratch.deallocate()
        outputScratch.deinitialize(count: scratchFrames)
        outputScratch.deallocate()
        gainPointer.deinitialize(count: 1)
        gainPointer.deallocate()
    }
}

private final class MonoRingBuffer {
    private let capacity: Int
    private let storage: UnsafeMutablePointer<Float>
    private var readIndex: UInt64 = 0
    private var writeIndex: UInt64 = 0

    init(capacity: Int) {
        self.capacity = max(capacity, 1)
        self.storage = UnsafeMutablePointer<Float>.allocate(capacity: self.capacity)
        self.storage.initialize(repeating: 0, count: self.capacity)
    }

    deinit {
        storage.deinitialize(count: capacity)
        storage.deallocate()
    }

    func write(_ source: UnsafePointer<Float>, frames: Int) {
        let frames = min(frames, capacity)
        if frames <= 0 { return }

        let availableSpace = capacity - Int(writeIndex - readIndex)
        if frames > availableSpace {
            readIndex += UInt64(frames - availableSpace)
        }

        for offset in 0..<frames {
            let index = Int((writeIndex + UInt64(offset)) % UInt64(capacity))
            storage[index] = source[offset]
        }
        writeIndex += UInt64(frames)
    }

    func read(into destination: UnsafeMutablePointer<Float>, frames: Int) {
        let frames = min(frames, capacity)
        guard frames > 0 else { return }

        let available = min(Int(writeIndex - readIndex), frames)
        for offset in 0..<available {
            let index = Int((readIndex + UInt64(offset)) % UInt64(capacity))
            destination[offset] = storage[index]
        }
        if available < frames {
            destination.advanced(by: available).initialize(repeating: 0, count: frames - available)
        }
        readIndex += UInt64(available)
    }
}

private let inputRenderCallback: AURenderCallback = {
    refCon,
    actionFlags,
    timeStamp,
    _,
    frameCount,
    _ in

    let state = Unmanaged<HALPlaythroughState>.fromOpaque(refCon).takeUnretainedValue()
    guard let inputUnit = state.inputUnit else { return noErr }

    let frames = min(Int(frameCount), state.scratchFrames)
    state.inputBufferList.pointee.mBuffers.mDataByteSize =
        UInt32(frames * MemoryLayout<Float>.size)
    state.inputBufferList.pointee.mBuffers.mData = UnsafeMutableRawPointer(state.inputScratch)

    let status = AudioUnitRender(
        inputUnit,
        actionFlags,
        timeStamp,
        1,
        UInt32(frames),
        state.inputBufferList
    )
    guard status == noErr else { return status }

    state.ring.write(state.inputScratch, frames: frames)
    return noErr
}

private let outputRenderCallback: AURenderCallback = {
    refCon,
    _,
    _,
    _,
    frameCount,
    ioData in

    guard let ioData else { return noErr }

    let state = Unmanaged<HALPlaythroughState>.fromOpaque(refCon).takeUnretainedValue()
    let frames = min(Int(frameCount), state.scratchFrames)
    state.ring.read(into: state.outputScratch, frames: frames)

    let gain = state.gainPointer.pointee
    let buffers = UnsafeMutableAudioBufferListPointer(ioData)
    for index in 0..<buffers.count {
        let channelCount = max(Int(buffers[index].mNumberChannels), 1)
        let sampleCount = Int(frameCount) * channelCount
        buffers[index].mDataByteSize = UInt32(sampleCount * MemoryLayout<Float>.size)

        guard let rawData = buffers[index].mData else { continue }
        let data = rawData.assumingMemoryBound(to: Float.self)
        for frame in 0..<frames {
            let sample = state.outputScratch[frame] * gain
            for channel in 0..<channelCount {
                data[(frame * channelCount) + channel] = sample
            }
        }
        if frames < Int(frameCount) {
            let start = frames * channelCount
            data.advanced(by: start).initialize(repeating: 0, count: sampleCount - start)
        }
    }

    return noErr
}

private func monoFloatFormat(sampleRate: Double) -> AudioStreamBasicDescription {
    nonInterleavedFloatFormat(sampleRate: sampleRate, channels: 1)
}

private func nonInterleavedFloatFormat(
    sampleRate: Double,
    channels: UInt32
) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription(
        mSampleRate: sampleRate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat
            | kAudioFormatFlagIsPacked
            | kAudioFormatFlagIsNonInterleaved,
        mBytesPerPacket: UInt32(MemoryLayout<Float>.size),
        mFramesPerPacket: 1,
        mBytesPerFrame: UInt32(MemoryLayout<Float>.size),
        mChannelsPerFrame: channels,
        mBitsPerChannel: UInt32(MemoryLayout<Float>.size * 8),
        mReserved: 0
    )
}

private func check(_ status: OSStatus, _ operation: String) throws {
    guard status == noErr else {
        throw AudioEngineError.audioUnitFailed(operation, status)
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

private func defaultInputDeviceID() throws -> AudioDeviceID {
    try defaultDeviceID(selector: kAudioHardwarePropertyDefaultInputDevice)
}

private func defaultOutputDeviceID() throws -> AudioDeviceID {
    try defaultDeviceID(selector: kAudioHardwarePropertyDefaultOutputDevice)
}

private func defaultDeviceID(selector: AudioObjectPropertySelector) throws -> AudioDeviceID {
    var address = AudioObjectPropertyAddress(
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var id = AudioDeviceID(0)
    var size = UInt32(MemoryLayout<AudioDeviceID>.size)
    let status = AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject),
        &address,
        0,
        nil,
        &size,
        &id
    )
    guard status == noErr, id != 0 else {
        throw AudioEngineError.deviceSetFailed(status == noErr ? -1 : status)
    }
    return id
}

private func nominalSampleRate(_ deviceID: AudioDeviceID) -> Double? {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyNominalSampleRate,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var sampleRate = Float64(0)
    var size = UInt32(MemoryLayout<Float64>.size)
    guard AudioObjectGetPropertyData(deviceID, &address, 0, nil, &size, &sampleRate) == noErr,
          sampleRate > 0
    else {
        return nil
    }
    return sampleRate
}

private func deviceBufferFrameSize(_ deviceID: AudioDeviceID) -> UInt32? {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyBufferFrameSize,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var frames = UInt32(0)
    var size = UInt32(MemoryLayout<UInt32>.size)
    guard AudioObjectGetPropertyData(deviceID, &address, 0, nil, &size, &frames) == noErr,
          frames > 0
    else {
        return nil
    }
    return frames
}
