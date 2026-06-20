# KaraokeMic v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a native macOS SwiftUI app that captures mic input and plays it back through the selected speaker in near-real-time for live karaoke use.

**Architecture:** A `@MainActor`-isolated `MicController` (`@Observable`) sits between SwiftUI and a `@MainActor`-protocol-gated `AVAudioEngine` wrapper. The signal path is `inputNode → mainMixerNode → outputNode` — no tap required. Device enumeration uses CoreAudio APIs directly. A `Scripts/run_dev.sh` creates a minimal `.app` bundle so `NSMicrophoneUsageDescription` is present for TCC (privacy permission) granting during development.

**Tech Stack:** Swift 6.3.2, SwiftUI, AVAudioEngine, AVFoundation (mic permission), CoreAudio, Swift Testing

## Global Constraints

- macOS 14+ (required for `@Observable` and modern CoreAudio constants)
- Swift 6 strict concurrency — no `@unchecked Sendable`, no `nonisolated(unsafe)` without justification
- No external SwiftPM dependencies — Apple frameworks only
- Single window, fixed width 360pt, height content-driven
- Audio signal path: mic in → AVAudioEngine.inputNode → mainMixerNode → outputNode → speaker
- Latency target: ≤15ms on wired devices
- `package` access level for all types shared between `KaraokeMicCore` and `KaraokeMic` executable targets
- `toggleRunning()` is `async` on the controller; SwiftUI buttons use `Task { await ... }`

---

## File Structure

```
Package.swift
version.env
IDEAS.md
Scripts/
  run_dev.sh                      — builds binary, wraps in .app with Info.plist, opens it
  compile_and_run.sh              — full packaging via package_app.sh (added in Task 4)
  package_app.sh                  — full bundle assembler from macos-spm-app-packaging skill (Task 4)
Sources/
  KaraokeMicCore/                 — library target (importable by tests)
    AudioEngineProtocol.swift     — @MainActor protocol, AudioEngineError, AudioEngine stub
    AudioDevice.swift             — AudioDevice struct + CoreAudio device enumeration (Task 3)
    AudioEngine.swift             — Concrete AVAudioEngine impl (Task 4)
    MicController.swift           — @Observable @MainActor controller
    ContentView.swift             — SwiftUI single-window view
  KaraokeMic/                     — executable target (entry point only)
    KaraokeMicApp.swift           — @main App, WindowGroup, injects MicController
Tests/
  KaraokeMicTests/
    FakeAudioEngine.swift         — Test double conforming to AudioEngineProtocol
    MicControllerTests.swift      — Unit tests for MicController
```

---

### Task 1: Project Scaffold

**Files:**
- Create: `Package.swift`
- Create: `version.env`
- Create: `IDEAS.md`
- Create: `Scripts/run_dev.sh`
- Create: `Sources/KaraokeMicCore/AudioEngineProtocol.swift` (complete — used by stub MicController)
- Create: `Sources/KaraokeMicCore/MicController.swift` (stub — replaced in Task 2)
- Create: `Sources/KaraokeMicCore/ContentView.swift` (stub — replaced in Task 5)
- Create: `Sources/KaraokeMic/KaraokeMicApp.swift`
- Create: `Tests/KaraokeMicTests/MicControllerTests.swift` (placeholder)

**Interfaces:**
- Produces: A `swift build`-able and `swift test`-able project; a stub window that opens via `Scripts/run_dev.sh`

- [ ] **Step 1: Write Package.swift**

```swift
// swift-tools-version: 6.2
import PackageDescription

let package = Package(
    name: "KaraokeMic",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "KaraokeMic", targets: ["KaraokeMic"]),
    ],
    targets: [
        .target(
            name: "KaraokeMicCore",
            path: "Sources/KaraokeMicCore"
        ),
        .executableTarget(
            name: "KaraokeMic",
            dependencies: ["KaraokeMicCore"],
            path: "Sources/KaraokeMic"
        ),
        .testTarget(
            name: "KaraokeMicTests",
            dependencies: ["KaraokeMicCore"],
            path: "Tests/KaraokeMicTests"
        ),
    ]
)
```

- [ ] **Step 2: Write AudioEngineProtocol.swift**

`Sources/KaraokeMicCore/AudioEngineProtocol.swift`:
```swift
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
```

- [ ] **Step 3: Write stub MicController**

`Sources/KaraokeMicCore/MicController.swift`:
```swift
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
```

- [ ] **Step 4: Write stub ContentView**

`Sources/KaraokeMicCore/ContentView.swift`:
```swift
import SwiftUI

package struct ContentView: View {
    @Environment(MicController.self) var controller

    package init() {}

    package var body: some View {
        Text("KaraokeMic — coming soon")
            .padding()
            .frame(width: 360)
    }
}
```

- [ ] **Step 5: Write the entry point**

`Sources/KaraokeMic/KaraokeMicApp.swift`:
```swift
import SwiftUI
import KaraokeMicCore

@main
struct KaraokeMicApp: App {
    @State private var controller = MicController()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(controller)
        }
        .windowResizability(.contentSize)
        .defaultSize(width: 360, height: 400)
    }
}
```

- [ ] **Step 6: Write placeholder test file**

`Tests/KaraokeMicTests/MicControllerTests.swift`:
```swift
import Testing
@testable import KaraokeMicCore
// Tests added in Task 2
```

- [ ] **Step 7: Write version.env and IDEAS.md**

`version.env`:
```
MARKETING_VERSION=0.1.0
BUILD_NUMBER=1
```

`IDEAS.md`:
```markdown
# KaraokeMic — Future Ideas

- In-app YouTube search and playback
- Voice effects (reverb, echo, pitch shift)
- Audio visualizer
- Recording the performance
```

- [ ] **Step 8: Write Scripts/run_dev.sh**

Create the `Scripts/` directory and write `Scripts/run_dev.sh`:
```bash
#!/usr/bin/env bash
# Builds KaraokeMic and opens it as a .app bundle with mic permission in Info.plist.
set -euo pipefail

APP_NAME="KaraokeMic"
APP_PATH="/tmp/${APP_NAME}.app"
CONTENTS="${APP_PATH}/Contents"

echo "==> Building..."
swift build -c debug

echo "==> Assembling bundle at ${APP_PATH}..."
rm -rf "${APP_PATH}"
mkdir -p "${CONTENTS}/MacOS" "${CONTENTS}/Resources"
cp ".build/debug/${APP_NAME}" "${CONTENTS}/MacOS/${APP_NAME}"
chmod +x "${CONTENTS}/MacOS/${APP_NAME}"

cat > "${CONTENTS}/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>KaraokeMic</string>
    <key>CFBundleIdentifier</key>
    <string>com.example.KaraokeMic</string>
    <key>CFBundleExecutable</key>
    <string>KaraokeMic</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>KaraokeMic needs microphone access to route your voice to the speaker.</string>
    <key>LSMinimumSystemVersion</key>
    <string>14.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

echo "==> Signing ad-hoc..."
codesign --force --sign - "${APP_PATH}"

echo "==> Killing any previous instance..."
pkill -x "${APP_NAME}" 2>/dev/null || true

echo "==> Launching..."
open "${APP_PATH}"
```

Make it executable:
```bash
chmod +x Scripts/run_dev.sh
```

- [ ] **Step 9: Verify build**

```bash
swift build
```
Expected:
```
Build complete!
```

- [ ] **Step 10: Verify tests run (0 tests)**

```bash
swift test
```
Expected:
```
Test run with 0 tests passed in X.XXX seconds.
```

- [ ] **Step 11: Commit**

```bash
git add Package.swift version.env IDEAS.md \
    Scripts/run_dev.sh \
    Sources/KaraokeMicCore/AudioEngineProtocol.swift \
    Sources/KaraokeMicCore/MicController.swift \
    Sources/KaraokeMicCore/ContentView.swift \
    Sources/KaraokeMic/KaraokeMicApp.swift \
    Tests/KaraokeMicTests/MicControllerTests.swift
git commit -m "feat: scaffold KaraokeMic SwiftPM project with stub UI"
```

---

### Task 2: AudioEngineProtocol + Tested MicController

**Files:**
- `Sources/KaraokeMicCore/AudioEngineProtocol.swift` — already complete from Task 1, no changes
- Modify: `Sources/KaraokeMicCore/MicController.swift` (replace stub with full implementation)
- Create: `Tests/KaraokeMicTests/FakeAudioEngine.swift`
- Modify: `Tests/KaraokeMicTests/MicControllerTests.swift` (full test suite)

**Interfaces:**
- Consumes: `AudioEngineProtocol`, `AudioEngineError` from Task 1
- Produces:
  - `MicController(engine: any AudioEngineProtocol)` — testable initializer
  - `MicController.toggleRunning() async`
  - `MicController.toggleMute()`
  - `MicController.gain: Float` with `didSet` → `engine.setGain`
  - `MicController.isMuted`, `.isRunning`, `.errorMessage`, `.estimatedLatencyMs`
  - `MicController.selectedInputDevice`, `.selectedOutputDevice` with `didSet` → `engine.setInput/OutputDevice`
  - `MicController.inputDevices: [AudioDevice]`, `.outputDevices: [AudioDevice]` (initially `[]`)
  - `FakeAudioEngine` test double tracking all calls

Note: `AudioDevice` is a placeholder struct defined at the bottom of `MicController.swift` in this task and moved to its own file in Task 3.

- [ ] **Step 1: Write the failing tests**

Replace `Tests/KaraokeMicTests/MicControllerTests.swift` entirely:

```swift
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
        controller.gain = 1.5
        #expect(fake.lastGain == 1.5)
    }

    @Test func configurationChange_stopsRunningAndSetsError() async throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        await controller.toggleRunning()  // start
        fake.onConfigurationChange?()
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
}
```

- [ ] **Step 2: Run — verify build failure**

```bash
swift test 2>&1 | head -20
```
Expected: compile error — `FakeAudioEngine` not found; `MicController(engine:)` not found; `AudioDevice` not found.

- [ ] **Step 3: Write FakeAudioEngine**

`Tests/KaraokeMicTests/FakeAudioEngine.swift`:
```swift
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

    func setInputDevice(_ deviceID: AudioDeviceID) throws { lastInputDeviceID = deviceID }
    func setOutputDevice(_ deviceID: AudioDeviceID) throws { lastOutputDeviceID = deviceID }
    func setGain(_ gain: Float) { lastGain = gain }
    func setMuted(_ muted: Bool) { lastMuted = muted }

    enum FakeEngineError: Error { case startFailed }
}
```

- [ ] **Step 4: Replace MicController stub with full implementation**

`Sources/KaraokeMicCore/MicController.swift`:
```swift
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
        // Implemented in Task 3 when AudioDevice.swift is added.
    }

    private func handleConfigurationChange() {
        engine.stop()
        isRunning = false
        errorMessage = "Audio device changed or disconnected. Re-select your devices and tap Start."
    }
}

// Placeholder — moved to AudioDevice.swift in Task 3.
package struct AudioDevice: Identifiable, Equatable, Hashable {
    package let id: AudioDeviceID
    package let name: String
    package let isBluetooth: Bool
}
```

- [ ] **Step 5: Run tests — verify all 8 pass**

```bash
swift test
```
Expected:
```
Test Suite 'All tests' passed.
  ✓ start_callsEngineStartAndSetsRunning
  ✓ stop_callsEngineStopAndClearsRunning
  ✓ startFailure_setsErrorMessageAndDoesNotRun
  ✓ mute_callsSetMutedTrueAndTogglesMuted
  ✓ gainChange_callsSetGain
  ✓ configurationChange_stopsRunningAndSetsError
  ✓ selectedInputDevice_callsEngineSetInputDevice
  ✓ selectedOutputDevice_callsEngineSetOutputDevice
  8 tests passed in X.XXX seconds.
```

- [ ] **Step 6: Commit**

```bash
git add Sources/KaraokeMicCore/MicController.swift \
    Tests/KaraokeMicTests/FakeAudioEngine.swift \
    Tests/KaraokeMicTests/MicControllerTests.swift
git commit -m "feat: add tested MicController with AudioEngineProtocol"
```

---

### Task 3: CoreAudio Device Enumeration

**Files:**
- Create: `Sources/KaraokeMicCore/AudioDevice.swift`
- Modify: `Sources/KaraokeMicCore/MicController.swift` (remove `AudioDevice` placeholder, fill `refreshDevices()`)
- Modify: `Tests/KaraokeMicTests/MicControllerTests.swift` (add smoke test for `refreshDevices`)

**Interfaces:**
- Consumes: `AudioDevice` placeholder in `MicController.swift` (remove it); `FakeAudioEngine.lastInputDeviceID` / `lastOutputDeviceID` already present from Task 2
- Produces:
  - `package struct AudioDevice: Identifiable, Equatable, Hashable` in `AudioDevice.swift`
  - `package func listInputDevices() -> [AudioDevice]`
  - `package func listOutputDevices() -> [AudioDevice]`
  - `MicController.refreshDevices()` populates `inputDevices`/`outputDevices` and auto-selects first

- [ ] **Step 1: Write the new failing test**

Add to `MicControllerTests` struct in `Tests/KaraokeMicTests/MicControllerTests.swift`:
```swift
    @Test func refreshDevices_doesNotCrashAndYieldsArrays() throws {
        let fake = FakeAudioEngine()
        let controller = MicController(engine: fake)
        controller.refreshDevices()
        // Hardware may be absent in CI; just verify no crash and the properties are typed correctly.
        _ = controller.inputDevices as [AudioDevice]
        _ = controller.outputDevices as [AudioDevice]
    }
```

- [ ] **Step 2: Run — all 9 tests should pass (no new failure yet)**

```bash
swift test
```
Expected: 9 tests pass. `refreshDevices()` currently does nothing — that's fine, the smoke test just verifies it doesn't crash.

- [ ] **Step 3: Create AudioDevice.swift with CoreAudio implementation**

`Sources/KaraokeMicCore/AudioDevice.swift`:
```swift
import CoreAudio

package struct AudioDevice: Identifiable, Equatable, Hashable {
    package let id: AudioDeviceID
    package let name: String
    package let isBluetooth: Bool
}

package func listInputDevices() -> [AudioDevice] {
    allDeviceIDs().compactMap { id in
        guard deviceHasStreams(id, scope: kAudioObjectPropertyScopeInput) else { return nil }
        return AudioDevice(id: id, name: deviceName(id), isBluetooth: deviceIsBluetooth(id))
    }
}

package func listOutputDevices() -> [AudioDevice] {
    allDeviceIDs().compactMap { id in
        guard deviceHasStreams(id, scope: kAudioObjectPropertyScopeOutput) else { return nil }
        return AudioDevice(id: id, name: deviceName(id), isBluetooth: deviceIsBluetooth(id))
    }
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

private func deviceName(_ id: AudioDeviceID) -> String {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioObjectPropertyName,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    // kAudioObjectPropertyName returns a +1-retained CFStringRef; use
    // Unmanaged so ARC ownership is explicit (avoids a leak and the
    // "forming pointer to CFString" warning).
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

private func deviceIsBluetooth(_ id: AudioDeviceID) -> Bool {
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioDevicePropertyTransportType,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var transport: UInt32 = 0
    var size = UInt32(MemoryLayout<UInt32>.size)
    guard AudioObjectGetPropertyData(id, &address, 0, nil, &size, &transport) == noErr else {
        return false
    }
    return transport == kAudioDeviceTransportTypeBluetooth
        || transport == kAudioDeviceTransportTypeBluetoothLE
}
```

- [ ] **Step 4: Remove the AudioDevice placeholder from MicController.swift**

Delete the entire block at the bottom of `Sources/KaraokeMicCore/MicController.swift`:
```swift
// Placeholder — moved to AudioDevice.swift in Task 3.
package struct AudioDevice: Identifiable, Equatable, Hashable {
    package let id: AudioDeviceID
    package let name: String
    package let isBluetooth: Bool
}
```

- [ ] **Step 5: Fill refreshDevices() in MicController.swift**

Replace the empty `refreshDevices()` body:
```swift
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
```

- [ ] **Step 6: Build and run tests**

```bash
swift build && swift test
```
Expected:
```
Build complete!
Test Suite 'All tests' passed.
  9 tests passed in X.XXX seconds.
```

- [ ] **Step 7: Commit**

```bash
git add Sources/KaraokeMicCore/AudioDevice.swift \
    Sources/KaraokeMicCore/MicController.swift \
    Tests/KaraokeMicTests/MicControllerTests.swift
git commit -m "feat: add CoreAudio device enumeration and MicController device management"
```

---

### Task 4: Real AudioEngine

**Files:**
- Create: `Sources/KaraokeMicCore/AudioEngine.swift`
- Modify: `Sources/KaraokeMicCore/AudioEngineProtocol.swift` (remove the `AudioEngine` placeholder class)
- Create: `Scripts/package_app.sh` (from macos-spm-app-packaging template, patched for mic)
- Create: `Scripts/compile_and_run.sh` (from macos-spm-app-packaging template)

**Interfaces:**
- Consumes: `AudioEngineProtocol`, `AudioEngineError` (Task 1); `AudioDeviceID` from CoreAudio
- Produces: Functional mic-to-speaker passthrough; async mic permission check on `start()`; input/output device switching via CoreAudio `AudioUnitSetProperty`; device-disappearance via `AVAudioEngineConfigurationChange`

- [ ] **Step 1: Create AudioEngine.swift**

`Sources/KaraokeMicCore/AudioEngine.swift`:
```swift
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
```

- [ ] **Step 2: Remove the AudioEngine placeholder from AudioEngineProtocol.swift**

In `Sources/KaraokeMicCore/AudioEngineProtocol.swift`, delete the entire block starting with `// Placeholder — deleted when AudioEngine.swift is created in Task 4.` through its closing `}`.

The file should end after the `AudioEngineError` enum's closing `}`.

- [ ] **Step 3: Build and run tests**

```bash
swift build && swift test
```
Expected:
```
Build complete!
Test Suite 'All tests' passed.
  9 tests passed in X.XXX seconds.
```

- [ ] **Step 4: Copy packaging scripts from the skill template**

```bash
cp .agents/skills/macos-spm-app-packaging/assets/templates/package_app.sh Scripts/package_app.sh
cp .agents/skills/macos-spm-app-packaging/assets/templates/compile_and_run.sh Scripts/compile_and_run.sh
chmod +x Scripts/package_app.sh Scripts/compile_and_run.sh
```

- [ ] **Step 5: Patch Scripts/package_app.sh — add NSMicrophoneUsageDescription**

In `Scripts/package_app.sh`, find the `cat > "$APP/Contents/Info.plist"` heredoc. Before `</dict>` add:

```xml
    <key>NSMicrophoneUsageDescription</key>
    <string>KaraokeMic needs microphone access to route your voice to the speaker.</string>
```

The end of the dict should look like:
```xml
    <key>GitCommit</key><string>${GIT_COMMIT}</string>
    <key>NSMicrophoneUsageDescription</key>
    <string>KaraokeMic needs microphone access to route your voice to the speaker.</string>
</dict>
</plist>
PLIST
```

- [ ] **Step 6: Integration test — launch the stub app via run_dev.sh**

```bash
bash Scripts/run_dev.sh
```
Expected: stub window ("KaraokeMic — coming soon") opens. No crash. Close it.

The full Start/Mute/device UI comes in Task 5. At this point, if you want to manually verify audio passthrough before the UI is ready, replace `toggleRunning()` with a direct `try await engine.start()` call temporarily — but that's optional.

- [ ] **Step 7: Commit**

```bash
git add Sources/KaraokeMicCore/AudioEngine.swift \
    Sources/KaraokeMicCore/AudioEngineProtocol.swift \
    Scripts/package_app.sh \
    Scripts/compile_and_run.sh
git commit -m "feat: implement real AudioEngine with AVAudioEngine, mic permission, and device switching"
```

---

### Task 5: ContentView — Full UI

**Files:**
- Modify: `Sources/KaraokeMicCore/ContentView.swift` (replace stub with full UI)

**Interfaces:**
- Consumes: All `MicController` properties and methods from Tasks 2–4; `AudioDevice.isBluetooth` from Task 3

- [ ] **Step 1: Replace ContentView stub**

`Sources/KaraokeMicCore/ContentView.swift`:
```swift
import SwiftUI

package struct ContentView: View {
    @Environment(MicController.self) var controller

    package init() {}

    package var body: some View {
        @Bindable var controller = controller

        VStack(alignment: .leading, spacing: 20) {
            // Input device picker
            VStack(alignment: .leading, spacing: 4) {
                Label("Microphone", systemImage: "mic")
                    .font(.headline)
                Picker("Microphone", selection: $controller.selectedInputDevice) {
                    Text("None").tag(Optional<AudioDevice>.none)
                    ForEach(controller.inputDevices) { device in
                        Text(device.name).tag(Optional(device))
                    }
                }
                .labelsHidden()
            }

            // Output device picker
            VStack(alignment: .leading, spacing: 4) {
                Label("Speaker", systemImage: "speaker.wave.2")
                    .font(.headline)
                Picker("Speaker", selection: $controller.selectedOutputDevice) {
                    Text("None").tag(Optional<AudioDevice>.none)
                    ForEach(controller.outputDevices) { device in
                        HStack {
                            Text(device.name)
                            if device.isBluetooth {
                                Image(systemName: "b.circle")
                                    .foregroundStyle(.secondary)
                            }
                        }
                        .tag(Optional(device))
                    }
                }
                .labelsHidden()
            }

            // Gain slider
            VStack(alignment: .leading, spacing: 4) {
                Label("Mic Volume", systemImage: "slider.horizontal.3")
                    .font(.headline)
                Slider(value: $controller.gain, in: 0...2, step: 0.05)
            }

            Divider()

            // Start/Stop + Mute
            HStack(spacing: 12) {
                Button {
                    Task { await controller.toggleRunning() }
                } label: {
                    Label(
                        controller.isRunning ? "Stop" : "Start",
                        systemImage: controller.isRunning ? "stop.circle.fill" : "play.circle.fill"
                    )
                    .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .tint(controller.isRunning ? .red : .accentColor)

                Button {
                    controller.toggleMute()
                } label: {
                    Label(
                        controller.isMuted ? "Unmute" : "Mute",
                        systemImage: controller.isMuted ? "mic.slash.fill" : "mic.fill"
                    )
                }
                .buttonStyle(.bordered)
                .disabled(!controller.isRunning)
            }

            // Latency readout + Bluetooth warning
            if controller.isRunning {
                latencyView
            }

            // Error message
            if let message = controller.errorMessage {
                Text(message)
                    .font(.callout)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(20)
        .frame(width: 360)
        .onAppear { controller.refreshDevices() }
    }

    @ViewBuilder
    private var latencyView: some View {
        let ms = Int(controller.estimatedLatencyMs)
        let bluetoothOutput = controller.selectedOutputDevice?.isBluetooth == true

        HStack(spacing: 6) {
            if bluetoothOutput {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.yellow)
                Text("Bluetooth output — latency will be high (\(ms) ms)")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            } else {
                Image(systemName: "waveform.circle.fill")
                    .foregroundStyle(.green)
                Text("\(ms) ms round-trip")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
        }
    }
}
```

- [ ] **Step 2: Build**

```bash
swift build
```
Expected:
```
Build complete!
```

- [ ] **Step 3: Run tests**

```bash
swift test
```
Expected:
```
Test Suite 'All tests' passed.
  9 tests passed in X.XXX seconds.
```

- [ ] **Step 4: Run the app and verify the golden path**

```bash
bash Scripts/run_dev.sh
```

Verify each of the following manually:

1. Device pickers list the Mac's connected audio hardware (built-in mic, built-in speakers, any USB/HDMI devices).
2. Gain slider moves from 0 to 2×.
3. Tap "Start" — if first launch, macOS shows a mic permission dialog; grant it. The button label changes to "Stop", the latency readout appears with a millisecond value.
4. Speak into the mic — your voice is audible through the selected speaker in near-real-time.
5. "Mute" button silences the voice without stopping the engine; "Unmute" restores it.
6. Adjust the gain slider while monitoring — volume changes in real-time.
7. "Stop" — engine stops, latency readout disappears.
8. If Bluetooth headphones are the selected output, the yellow Bluetooth warning appears in the latency row.
9. Unplug the selected audio device while the engine is running — the error message appears and `isRunning` resets to false.

- [ ] **Step 5: Commit**

```bash
git add Sources/KaraokeMicCore/ContentView.swift
git commit -m "feat: complete ContentView UI with device pickers, gain slider, and latency readout"
```

---

## Self-Review

### 1. Spec Coverage

| Spec requirement | Task |
|---|---|
| Mic in → speaker out passthrough | Task 4 (`AudioEngine.start()`, `inputNode → mainMixerNode → outputNode`) |
| Lowest practical latency (5–15ms on wired) | Task 4 (`setPreferredBufferSize`, 256-frame IO buffer) |
| Input device picker | Task 5 (`ContentView` Picker bound to `selectedInputDevice`) |
| Output device picker | Task 5 (`ContentView` Picker bound to `selectedOutputDevice`) |
| Mic gain slider | Tasks 2 + 5 (`gain` didSet → `setGain`, Slider 0–2×) |
| Start/Stop toggle | Tasks 2 + 5 (`toggleRunning() async`, Button with Task wrapper) |
| Mute button | Tasks 2 + 5 (`toggleMute()`, disabled when not running) |
| Latency readout | Tasks 2 + 5 (`estimatedLatencyMs`, `latencyView`) |
| Bluetooth warning | Tasks 3 + 5 (`AudioDevice.isBluetooth`, yellow triangle in `latencyView`) |
| Mic permission denied → clear prompt | Tasks 1 + 4 (`AudioEngineError.microphonePermissionDenied` with System Settings guidance) |
| Device disappears mid-session | Task 4 (`AVAudioEngineConfigurationChange` → `onConfigurationChange` → `handleConfigurationChange`) |
| Engine fails to start → show error, stay stopped | Task 2 (`toggleRunning` catch block sets `errorMessage`) |
| Single window, 360pt fixed width | Tasks 1 + 5 (`WindowGroup .defaultSize`, `.frame(width: 360)`) |
| No song playback (non-goal) | Not implemented ✓ |
| INFO.plist + NSMicrophoneUsageDescription | Tasks 1 + 4 (`run_dev.sh` inline, `package_app.sh` patched) |

### 2. Placeholder Scan

No TBDs, no "implement later", no "handle edge cases", no "similar to Task N". All code blocks are complete and runnable.

### 3. Type Consistency

- `AudioDevice` defined once in `AudioDevice.swift` (Task 3). Placeholder in `MicController.swift` removed in same task. `FakeAudioEngine` test fixture in Task 2 uses `AudioDeviceID` directly — no `AudioDevice` needed there since the fields are typed as `AudioDeviceID`.
- `AudioEngineError` defined in `AudioEngineProtocol.swift` (Task 1). Referenced in `AudioEngine.start()` (Task 4) and `toggleRunning` catch (Task 2).
- `AudioEngine` stub in `AudioEngineProtocol.swift` (Task 1) deleted in Task 4 when real `AudioEngine.swift` is added.
- `toggleRunning() async` consistent across: protocol (N/A — not on protocol), `MicController` (Task 2), tests (`await controller.toggleRunning()`), `ContentView` (`Task { await controller.toggleRunning() }`).
- `FakeAudioEngine.lastInputDeviceID` / `lastOutputDeviceID` declared in Task 2 (`FakeAudioEngine.swift`) and referenced in Task 2 tests (`MicControllerTests.swift`).
- `MicController.init(engine:)` declared in Task 2, used in every test.
