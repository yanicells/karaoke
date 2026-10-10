# KaraokeMic

A small native macOS app for hearing your microphone through a selected speaker while you sing along. Built with Rust, GPUI 0.2.2, and native macOS audio APIs.

Choose your microphone and speaker, adjust mic volume, then press **Start monitoring**. **Mute** silences your voice without stopping the route. Play YouTube or other music separately in your browser; KaraokeMic does not control or capture it.

Requires macOS 14 or later, stable Rust, and Xcode with its command-line tools selected (`sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer`). Builds target the host architecture.

## Build and run

```sh
cargo build --locked --package karaoke-mic --bin KaraokeMic
Scripts/run_dev.sh
```

The development script packages and opens a debug app at `dist/KaraokeMic.app`. Launch the bundle to request microphone access: running the bare Cargo executable does not provide the app's permission usage description.

## Package and install

```sh
Scripts/package_app.sh                    # Signed release: dist/KaraokeMic.app
open dist/KaraokeMic.app                  # Run the release bundle
Scripts/install_spotlight_app.sh          # Install in ~/Applications and launch
Scripts/install_spotlight_app.sh --no-launch
Scripts/install_spotlight_app.sh --no-launch --reset-mic-permission
```

Packaging reuses Cargo's build cache, validates a staged bundle, and then replaces the previous output. Installation also validates its staged copy before replacing the installed app and registering it with LaunchServices and Spotlight. `--reset-mic-permission` explicitly resets only `com.karaokemic.KaraokeMic`; the next bundled launch can ask again. Debug and release builds use the same bundle identifier.

Local bundles are ad-hoc signed and unsandboxed, with a normal Dock presence and native menus. They are not notarized distribution builds. An ad-hoc signature identifies a specific build, so macOS may require microphone permission again after a rebuild. A consistent Developer ID signing identity is needed for durable distribution identity.

## Checks

```sh
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
Scripts/validate_bundle.sh                # After packaging; does not launch
dist/KaraokeMic.app/Contents/MacOS/KaraokeMic --list-devices
dist/KaraokeMic.app/Contents/MacOS/KaraokeMic --probe-muted
```

CI runs the build checks and packages a release app on macOS. The explicit muted probe requires existing microphone permission and verifies real audio callbacks without playing your voice. Optional arguments are `--probe-muted input-id output-id duration-ms`. It never requests or resets permission. Hardware routing, permission prompts, and audible behavior need manual verification; see the [port checklist](docs/rust-port.md).

## Limits and permissions

- Latency estimates include device/stream latency, safety offsets, current buffers, and software conversion/queue delay. They are not physical microphone-to-speaker measurements. Bluetooth adds delay; wired outputs usually work better.
- Audio setup runs on a worker. Native callbacks use preallocated buffers, a bounded atomic queue, and clock-aware sample-rate conversion. Hardware sample rates and shared device buffer sizes are left unchanged.
- Microphone access is managed in **System Settings → Privacy & Security → Microphone**. Enable KaraokeMic, then restart it. Start or device-selection failures appear in the app.
- If a selected device disconnects or changes configuration, monitoring stops; select available devices and press Start again.
- YouTube playback stays independent. There is no song playback, recording, voice effects, or digital song mixing in this version.
- macOS only. Keep speaker volume modest and the microphone away from the speaker to avoid feedback.

The icon source is [assets/logo.svg](assets/logo.svg); the included PNG and ICNS are generated with `python3 Scripts/generate_icon.py` using Python's standard library and macOS `sips`/`iconutil`. No image-generation service or runtime image tooling is required.

Keyboard: **Tab** moves between controls; arrows select devices or adjust volume; **Enter/Space** activates the focused control; **M** mutes while running; **Esc** closes a picker; **⌘R** refreshes devices; **⌘Q** quits.
