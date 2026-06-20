# KaraokeMic — v1 Design

**Date:** 2026-06-20
**Status:** Approved

## Summary

KaraokeMic is a native macOS app that turns a Mac into a low-latency karaoke
microphone. It captures the selected microphone and plays it back through the
selected speaker in near-real-time, so the user can sing along to a YouTube
video they have open in a separate browser tab.

The app does **not** play or control the song in v1. Song playback is whatever
the user has running in their browser. KaraokeMic's only job is the live voice
path: mic in, speaker out, as fast as possible.

## Goals

- Recreate the "real karaoke" feel: you sing, you hear yourself in the room
  almost instantly.
- Lowest practical mic-to-speaker latency on a Mac (~5–15ms on wired gear).
- Simple single-window UI with the controls needed to pick devices and tune
  the voice level.

## Non-Goals (v1)

- Playing, searching, or embedding YouTube / any song source.
- Mixing the song audio with the voice digitally (not possible from a YouTube
  embed anyway — cross-origin protection blocks tapping that audio stream).
- Voice effects, recording, or visualizers (tracked in `IDEAS.md`).

## Technology Choice

**Swift + SwiftUI, audio via `AVAudioEngine`.**

Rationale: `AVAudioEngine` sits directly on top of CoreAudio, the shortest
software path from mic to speaker on macOS. A small IO buffer
(`setPreferredIOBufferDuration`) gets latency into the single-digit-to-low-teens
milliseconds range on wired gear. Alternatives considered: Tauri/Rust (`cpal`)
adds an abstraction layer over CoreAudio and cross-platform overhead we don't
need (Mac-only); Electron has the worst latency and weight. Both rejected.

## Architecture

Two layers with a thin observable controller between them.

### 1. Audio engine (no UI knowledge)

An `AVAudioEngine` wrapper that owns the signal path:

```
Mic (selected input)
  → AVAudioEngine.inputNode
  → mixer node (applies gain / mute)
  → AVAudioEngine.outputNode (selected output device)
  → Speaker
```

Responsibilities:
- Configure low IO buffer duration for latency.
- Start / stop the engine.
- Apply mic gain and mute via the mixer node.
- Select input and output devices (via CoreAudio device IDs on the engine's
  audio units).
- Report an estimated round-trip latency value.

The engine is hidden behind a protocol so the controller can be unit-tested
against a fake.

### 2. UI layer (SwiftUI, no audio logic)

A single window observing an `@Observable` controller. The controller exposes
engine state (running, muted, gain, device lists, selected devices, latency)
and forwards user actions to the engine.

## v1 Window Controls

- **Input device picker** — dropdown of available microphones.
- **Output device picker** — dropdown of available speakers/outputs.
- **Mic gain slider** — adjusts voice volume in the output.
- **Start/Stop toggle + Mute button** — primary control to begin/end monitoring
  and quickly silence the mic.
- **Latency readout** — shows estimated latency in ms; warns when the selected
  output appears to be Bluetooth (which adds large lag and breaks the feel).

## Audio / Mixing Note

YouTube audio plays out the system output on its own. KaraokeMic's voice also
plays out the chosen output. The two blend acoustically at the speaker — there
is no digital mixing. This is by design and keeps the voice path independent of
(and unaffected by) whatever is playing the song.

## Error Handling

- **Mic permission denied** → clear prompt explaining how to enable microphone
  access in System Settings; app stays in a stopped, safe state.
- **Device disappears mid-session** (e.g. earphones unplugged) → stop the engine
  cleanly, surface a message, let the user re-pick a device.
- **Engine fails to start** → show the error, remain stopped, do not crash.

## Testing

- Unit-test controller logic (device enumeration handling, gain/mute state,
  selection) against a faked engine protocol.
- The audio path itself (actual latency and feel) is verified manually — it is
  subjective and hardware-dependent.

## Future Ideas

A simple `IDEAS.md` file at the repo root will track possible future additions:

- In-app YouTube search and playback.
- Voice effects (reverb, echo, pitch).
- Audio visualizer.
- Recording the performance.
