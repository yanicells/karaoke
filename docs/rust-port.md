# Rust / GPUI port

The Rust application replaces the shipped SwiftUI app with GPUI 0.2.2 and direct native macOS audio FFI. The Cargo package is `karaoke-mic`; its app executable is `KaraokeMic`. Historical design documents remain in `docs/superpowers`; this checklist follows the shipped Swift implementation rather than proposed features in `IDEAS.md` or the original AVAudioEngine design.

## Feature acceptance checklist

Complete these checks against the final Rust bundle. Unchecked items are pending verification, not claims of missing implementation.

- [ ] One native window presents microphone and speaker selection, volume, Start/Stop, Mute/Unmute, route status, and errors.
- [ ] Enumerate CoreAudio input/output devices by stream availability, using native device names and IDs.
- [ ] Identify Bluetooth/Bluetooth LE outputs and surface the latency warning.
- [ ] Identify built-in and Continuity Capture devices, including the `ccap`, `ccwd`, and `ccwl` transports.
- [ ] Preserve preferred microphone selection: Continuity Capture, then a name containing `cell`, then a non-built-in input, then the first input.
- [ ] Preserve preferred speaker selection: a built-in device named speaker, then a device named speaker, then a non-Bluetooth/non-Continuity output, then the first output.
- [ ] Device selection routes the microphone and speaker independently; changing either while running reconfigures the live route.
- [ ] Start requests microphone permission from the bundled app, then starts monitoring; Stop releases both audio units and clears the latency readout.
- [ ] Default mic gain is 1.0; the slider spans 0.0–2.0 in 0.05 steps and updates live audio.
- [ ] Mute is disabled while stopped. Mute applies zero gain while monitoring; Unmute restores the chosen gain. Changing gain while muted stays silent.
- [ ] Preserve mute and gain state across Stop/Start and device reconfiguration within a session.
- [ ] Capture mono floating-point microphone audio and reproduce it on all selected output channels through native HAL audio units.
- [ ] Configure each audio unit at its own device's native sample rate; bridge rates and independent clocks with preallocated adaptive bandlimited conversion. Supply silence while priming or recovering from underflow and bound accumulated backlog.
- [ ] Estimate latency using each device's rate, device/stream latency, safety offsets, buffer contributions, and software queue/filter delay; reset it while stopped and label it as an estimate.
- [ ] Permission denial/restriction, missing devices, audio-unit failures, and route-change failures surface useful messages without crashing or falsely reporting running state.
- [ ] A selected device's configuration change/disconnection stops monitoring, releases resources, clears latency, and offers recovery by reselecting devices and starting again.
- [ ] Browser/YouTube audio continues independently; KaraokeMic neither plays songs nor captures browser audio.
- [ ] Closing/quitting follows normal macOS application behavior and releases the native audio resources.
- [ ] Normal Dock, native application menus, and microphone icon work when launching the bundle through Finder/LaunchServices.

There was no saved performance history or recording database in the shipped app. Preserve repository history and the historical design/plan documents; persistent settings, song controls, effects, visualizers, and recording are outside this port's feature baseline.

## Bundle and installation

- [ ] Production output is `dist/KaraokeMic.app`, containing a native host-architecture `Contents/MacOS/KaraokeMic` executable.
- [ ] Bundle identity is `com.karaokemic.KaraokeMic` for debug, release, and Spotlight installation.
- [ ] Minimum macOS is 14.0, principal class is `NSApplication`, and `LSUIElement` is false.
- [ ] `NSMicrophoneUsageDescription` retains: “KaraokeMic needs microphone access to route your voice to the speaker.”
- [ ] The included `.icns` is copied into Resources and referenced by `CFBundleIconFile`.
- [ ] Local ad-hoc signature verifies with `codesign --verify --strict`.
- [ ] Packaging and installation stage and validate replacements before moving the existing app aside; a failed final move restores the old bundle.
- [ ] `install_spotlight_app.sh --no-launch` registers `~/Applications/KaraokeMic.app` without launching or quitting a running app.
- [ ] `--reset-mic-permission` resets only this stable bundle identity when explicitly supplied; ordinary packaging and installation leave microphone permission alone.

Local packaging is unsandboxed, matching the previous app. The bundle identifier is consistent, but ad-hoc signing produces a build-specific designated requirement; microphone grants may need renewal after rebuilding. A future sandboxed build needs `com.apple.security.app-sandbox` and `com.apple.security.device.audio-input` entitlements and separate validation. Permission status can be inspected without requesting access; permission requests must run inside a bundle containing the usage description to avoid macOS terminating the process.

The route never changes shared hardware sample rates or buffer sizes. This avoids leaving another app's device settings altered after cancellation or a delayed driver property change. Smaller hardware buffers, where supported by the user's interface, can lower latency; the app bounds its own software queue independently.

## Verification responsibilities

CI runs formatting, all-target check, strict Clippy, unit tests, release packaging, and static bundle/signature validation. It does not request microphone access or install into a user's Applications directory.

Manual acceptance needs a bundled launch and real devices: permission granted/denied, Start/Stop, gain/mute, live device switching, disconnection recovery, Bluetooth warning, independent browser playback, and Dock/menu/Spotlight behavior. The displayed latency is not a measured round-trip value; verify audible monitoring separately. Never reset permission or overwrite an installed app as an incidental build check.

### Local verification on 2026-10-10

- All 34 unit tests passed, covering controller preferences/state transitions, worker cancellation/shutdown/disconnection, permission-block ownership, channel layouts, mute, bounded queues, clock drift, callback jitter, and resampling alias rejection.
- Formatting, all-target Cargo check, strict Clippy, and `git diff --check` passed.
- Apple Silicon and Intel release executables compiled successfully. Both the native Apple Silicon bundle and a universal `arm64 x86_64` bundle passed architecture, metadata/icon, and signature validation. Packaging argument checks rejected empty, unsupported, and duplicate architectures and conflicting debug/release options before replacing the bundle.
- A native window screenshot confirmed the text, microphone logo, device selections, gain slider, and Start/Mute controls render. The initial missing text/logo issue was fixed by enabling GPUI's font renderer and supplying embedded assets.
- Live discovery found a Continuity phone microphone, built-in microphone and speakers, and a virtual audio device.
- After the user granted access in the release app, silent probes launched through LaunchServices passed for both the phone microphone → built-in speakers and built-in microphone → built-in speakers. Each three-second run processed 143,872 input frames and 142,848 output frames, with zero underruns, overruns, or backlog resets. The probes changed gain while muted and stopped/disposed each route on completion.
- Estimated latency was 185.58 ms for the phone route and 100.02 ms for the built-in route. These are software/device estimates, not measured physical latency or a benchmark against the Swift app.
- Direct executable launches continued to report `not-determined` because they used a different microphone permission context. The README now launches the probe through `open` to use the app's grant.
- Audible gain/mute behavior, UI device switching, hardware disconnection/recovery, Bluetooth, and physical latency still need manual acceptance; the checklist above remains open for those checks.

These results establish implementation and automated verification, not completed hardware acceptance. No installed app was overwritten and no microphone permission was reset.

## Packaging interfaces

`Scripts/package_app.sh [release|debug]` always produces `dist/KaraokeMic.app`. It builds only `karaoke-mic` / `KaraokeMic` with Cargo's cache, sets `MACOSX_DEPLOYMENT_TARGET=14.0`, and discovers the executable from Cargo's JSON artifact stream, including custom target directories. Set `ARCHES='arm64 x86_64'` for a universal bundle, or either architecture for a single slice; requested slices are checked before signing. Install the corresponding Rust targets before cross-compiling. `version.env` supplies marketing version and build number. `Scripts/validate_bundle.sh [app-path]` checks identity, permission metadata, icon, executable format, and signature without launching.

`Scripts/run_dev.sh` launches a debug bundle; `Scripts/compile_and_run.sh [--debug] [--test] [--release-universal | --release-arches='arm64 x86_64']` packages and opens the app after optional tests. Release architecture flags cannot be combined with `--debug`. Production builds default to release. The checked-in icon is reused during packaging; regeneration is an explicit developer task.
