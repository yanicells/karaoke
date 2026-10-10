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

## Packaging interfaces

`Scripts/package_app.sh [release|debug]` always produces `dist/KaraokeMic.app`. It builds only `karaoke-mic` / `KaraokeMic` with Cargo's cache, sets `MACOSX_DEPLOYMENT_TARGET=14.0`, and discovers the executable from Cargo's JSON artifact stream, including custom target directories. `version.env` supplies marketing version and build number. `Scripts/validate_bundle.sh [app-path]` checks identity, permission metadata, icon, executable format, and signature without launching.

`Scripts/run_dev.sh` launches a debug bundle; `Scripts/compile_and_run.sh [--debug] [--test]` packages and opens the app after optional tests. Production builds default to release. The checked-in icon is reused during packaging; regeneration is an explicit developer task.
