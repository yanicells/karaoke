import CoreAudio

package struct AudioDevice: Identifiable, Equatable, Hashable {
    package let id: AudioDeviceID
    package let name: String
    package let isBluetooth: Bool
    package let isBuiltIn: Bool
    package let isContinuityCapture: Bool

    package init(
        id: AudioDeviceID,
        name: String,
        isBluetooth: Bool,
        isBuiltIn: Bool = false,
        isContinuityCapture: Bool = false
    ) {
        self.id = id
        self.name = name
        self.isBluetooth = isBluetooth
        self.isBuiltIn = isBuiltIn
        self.isContinuityCapture = isContinuityCapture
    }
}

package func listInputDevices() -> [AudioDevice] {
    allDeviceIDs().compactMap { id in
        guard deviceHasStreams(id, scope: kAudioObjectPropertyScopeInput) else { return nil }
        return audioDevice(id)
    }
}

package func listOutputDevices() -> [AudioDevice] {
    allDeviceIDs().compactMap { id in
        guard deviceHasStreams(id, scope: kAudioObjectPropertyScopeOutput) else { return nil }
        return audioDevice(id)
    }
}

package func audioDeviceExists(_ id: AudioDeviceID) -> Bool {
    allDeviceIDs().contains(id)
}

private func allDeviceIDs() -> [AudioDeviceID] {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyDevices,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var size: UInt32 = 0
    guard AudioObjectGetPropertyDataSize(
        AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size
    ) == noErr else { return [] }
    let count = Int(size) / MemoryLayout<AudioDeviceID>.size
    var ids = [AudioDeviceID](repeating: 0, count: count)
    guard AudioObjectGetPropertyData(
        AudioObjectID(kAudioObjectSystemObject), &address, 0, nil, &size, &ids
    ) == noErr else { return [] }
    return ids
}

private func audioDevice(_ id: AudioDeviceID) -> AudioDevice {
    let transport = deviceTransportType(id)
    return AudioDevice(
        id: id,
        name: deviceName(id),
        isBluetooth: transport == kAudioDeviceTransportTypeBluetooth
            || transport == kAudioDeviceTransportTypeBluetoothLE,
        isBuiltIn: transport == kAudioDeviceTransportTypeBuiltIn,
        isContinuityCapture: continuityCaptureTransportTypes.contains(transport)
    )
}

private func deviceName(_ id: AudioDeviceID) -> String {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioObjectPropertyName,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var nameUnmanaged: Unmanaged<CFString>?
    var size = UInt32(MemoryLayout<Unmanaged<CFString>?>.size)
    let status = AudioObjectGetPropertyData(id, &address, 0, nil, &size, &nameUnmanaged)
    guard status == noErr, let ref = nameUnmanaged else { return "" }
    return ref.takeRetainedValue() as String
}

private func deviceHasStreams(_ id: AudioDeviceID, scope: AudioObjectPropertyScope) -> Bool {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyStreams,
        mScope: scope,
        mElement: kAudioObjectPropertyElementMain
    )
    var size: UInt32 = 0
    return AudioObjectGetPropertyDataSize(id, &address, 0, nil, &size) == noErr && size > 0
}

private func deviceTransportType(_ id: AudioDeviceID) -> UInt32 {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyTransportType,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var transport: UInt32 = 0
    var size = UInt32(MemoryLayout<UInt32>.size)
    guard AudioObjectGetPropertyData(id, &address, 0, nil, &size, &transport) == noErr else {
        return kAudioDeviceTransportTypeUnknown
    }
    return transport
}

private let continuityCaptureTransportTypes: Set<UInt32> = [
    fourCharCode("ccap"),
    fourCharCode("ccwd"),
    fourCharCode("ccwl"),
]

private func fourCharCode(_ value: String) -> UInt32 {
    value.utf8.reduce(UInt32(0)) { result, byte in
        (result << 8) | UInt32(byte)
    }
}
