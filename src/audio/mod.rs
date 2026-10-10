//! macOS microphone playthrough. Construct and own AudioEngine on the dedicated
//! audio worker; permission and device setup may block. UI gain/mute updates go
//! through that worker and become atomic callback controls.
#[cfg(target_os = "macos")]
mod devices;
mod dsp;
#[cfg(target_os = "macos")]
mod ffi;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod permission;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: u32,
    pub name: String,
    pub is_bluetooth: bool,
    pub is_built_in: bool,
    pub is_continuity: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Devices {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

/// Cumulative counters for the current route. Silence during initial buffering
/// is not an underrun; an underrun means an already-playing route ran out of data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AudioDiagnostics {
    pub input_callbacks: u64,
    pub output_callbacks: u64,
    pub input_frames: u64,
    pub output_frames: u64,
    pub underruns: u64,
    pub overruns: u64,
    pub backlog_resets: u64,
    pub queued_frames: usize,
}

#[cfg(target_os = "macos")]
pub use devices::devices;
#[cfg(target_os = "macos")]
pub use macos::AudioEngine;
#[cfg(target_os = "macos")]
pub use permission::permission_status;
