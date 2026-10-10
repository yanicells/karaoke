use super::devices::{self, check, property};
use super::dsp::{Frame, Resampler, Ring, clean_gain, clean_sample};
use super::ffi::*;
use super::permission;
use super::{AudioDevice, AudioDiagnostics};
use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const CAPACITY: usize = 16_384;
const MAX_CHANNELS: usize = 64;
const ERROR_INPUT_RENDER: u32 = 1;
const ERROR_INPUT_CAPACITY: u32 = 2;
const ERROR_OUTPUT_CAPACITY: u32 = 3;
const ERROR_INPUT_FORMAT: u32 = 4;
const ERROR_OUTPUT_FORMAT: u32 = 5;
const ERROR_REENTRANT: u32 = 6;

fn priming_target(input_buffer: u32, output_buffer: u32, ratio: f64, taps: usize) -> usize {
    // Input and output callbacks can cross in phase by a full input block.
    // An output slice plus filter alone can starve before the next input arrives.
    input_buffer as usize + (output_buffer as f64 * ratio).ceil() as usize + taps
}

struct Shared {
    ring: Ring,
    enabled: AtomicBool,
    target_gain: AtomicU32,
    error: AtomicU64,
    reset: AtomicBool,
    input_clock: AtomicU64,
    last_input: AtomicU64,
    last_output: AtomicU64,
    input_callbacks: AtomicU64,
    output_callbacks: AtomicU64,
    input_frames: AtomicU64,
    output_frames: AtomicU64,
    underruns: AtomicU64,
    overruns: AtomicU64,
    backlog_resets: AtomicU64,
}
impl Shared {
    fn new(capacity: usize, gain: f32, muted: bool) -> Self {
        let now = unsafe { mach_absolute_time() };
        Self {
            ring: Ring::new(capacity),
            enabled: AtomicBool::new(false),
            target_gain: AtomicU32::new(if muted { 0.0f32 } else { clean_gain(gain) }.to_bits()),
            error: AtomicU64::new(0),
            reset: AtomicBool::new(false),
            input_clock: AtomicU64::new(1.0f64.to_bits()),
            last_input: AtomicU64::new(now),
            last_output: AtomicU64::new(now),
            input_callbacks: AtomicU64::new(0),
            output_callbacks: AtomicU64::new(0),
            input_frames: AtomicU64::new(0),
            output_frames: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            overruns: AtomicU64::new(0),
            backlog_resets: AtomicU64::new(0),
        }
    }
    // The worker owns gain/mute; callbacks read one complete effective value.
    fn publish_gain(&self, gain: f32, muted: bool) {
        let target = if muted { 0.0 } else { gain };
        self.target_gain.store(target.to_bits(), Ordering::Relaxed);
    }
    fn fail(&self, kind: u32, status: Status) {
        let value = ((kind as u64) << 32) | (status as u32 as u64);
        let _ = self
            .error
            .compare_exchange(0, value, Ordering::Relaxed, Ordering::Relaxed);
        self.enabled.store(false, Ordering::Release);
    }
    fn diagnostics(&self) -> AudioDiagnostics {
        AudioDiagnostics {
            input_callbacks: self.input_callbacks.load(Ordering::Relaxed),
            output_callbacks: self.output_callbacks.load(Ordering::Relaxed),
            input_frames: self.input_frames.load(Ordering::Relaxed),
            output_frames: self.output_frames.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
            backlog_resets: self.backlog_resets.load(Ordering::Relaxed),
            queued_frames: self.ring.queued(),
        }
    }
}

/// Estimate actual frames/second from CoreAudio's sample/host timestamp pair.
/// Host timestamps describe the device timeline, rather than callback scheduling.
/// This also works when a driver omits mRateScalar. Discontinuities reset the anchor.
struct Clock {
    rate: f64,
    seconds_per_tick: f64,
    anchor_sample: f64,
    anchor_host: u64,
    last_sample: f64,
    last_host: u64,
    estimate: f64,
}
impl Clock {
    fn new(rate: f64, seconds_per_tick: f64) -> Self {
        Self {
            rate,
            seconds_per_tick,
            anchor_sample: 0.0,
            anchor_host: 0,
            last_sample: 0.0,
            last_host: 0,
            estimate: 1.0,
        }
    }
    fn update(&mut self, time: &Timestamp) -> bool {
        if time.flags & 3 != 3 || !time.sample.is_finite() || time.host == 0 {
            return false;
        }
        let discontinuity = self.last_host != 0
            && (time.host <= self.last_host
                || time.sample < self.last_sample
                || (time.host - self.last_host) as f64 * self.seconds_per_tick > 0.5);
        if self.anchor_host == 0 || discontinuity {
            self.anchor_sample = time.sample;
            self.anchor_host = time.host;
            self.estimate = 1.0;
        } else {
            let seconds = (time.host - self.anchor_host) as f64 * self.seconds_per_tick;
            if seconds >= 0.25 {
                let measured = (time.sample - self.anchor_sample) / (seconds * self.rate);
                if measured.is_finite() && (0.98..=1.02).contains(&measured) {
                    self.estimate += 0.1 * (measured - self.estimate);
                }
                self.anchor_sample = time.sample;
                self.anchor_host = time.host;
            }
        }
        self.last_sample = time.sample;
        self.last_host = time.host;
        discontinuity
    }
}

struct Scratch {
    samples: Box<[f32]>,
    list: Box<[u64]>,
    channels: usize,
}
impl Scratch {
    fn new(channels: usize) -> Self {
        let bytes = offset_of!(BufferList, buffers) + channels * size_of::<Buffer>();
        Self {
            samples: vec![0.0; channels * CAPACITY].into_boxed_slice(),
            list: vec![0; bytes.div_ceil(8)].into_boxed_slice(),
            channels,
        }
    }
    fn prepare(&mut self, frames: usize) -> *mut BufferList {
        let list = self.list.as_mut_ptr().cast::<BufferList>();
        // list has exact channel capacity and samples were fully allocated before start.
        unsafe {
            (*list).count = self.channels as u32;
            let buffers = ptr::addr_of_mut!((*list).buffers).cast::<Buffer>();
            for channel in 0..self.channels {
                *buffers.add(channel) = Buffer {
                    channels: 1,
                    bytes: (frames * 4) as u32,
                    data: self.samples.as_mut_ptr().add(channel * CAPACITY).cast(),
                };
            }
        }
        list
    }
    fn valid(&self, frames: usize) -> bool {
        let list = self.list.as_ptr().cast::<BufferList>();
        unsafe {
            if (*list).count as usize != self.channels {
                return false;
            }
            let buffers = ptr::addr_of!((*list).buffers).cast::<Buffer>();
            for channel in 0..self.channels {
                let buffer = &*buffers.add(channel);
                if buffer.channels != 1
                    || buffer.bytes < (frames * 4) as u32
                    || buffer.data
                        != self
                            .samples
                            .as_ptr()
                            .add(channel * CAPACITY)
                            .cast_mut()
                            .cast()
                {
                    return false;
                }
            }
        }
        true
    }
    // Select channel 0 even when a driver requires native multichannel capture.
    fn mono_frame(&self, index: usize) -> Frame {
        let sample = clean_sample(self.samples[index]);
        Frame {
            left: sample,
            right: sample,
        }
    }
}
struct InputData {
    unit: Unit,
    scratch: Scratch,
    clock: Clock,
}
struct InputState {
    shared: Arc<Shared>,
    busy: AtomicBool,
    data: UnsafeCell<InputData>,
}
struct OutputData {
    converter: Resampler,
    clock: Clock,
    gain: f32,
}
struct OutputState {
    shared: Arc<Shared>,
    busy: AtomicBool,
    data: UnsafeCell<OutputData>,
    channels: usize,
    ratio: f64,
    rate: f64,
    target: usize,
    max_backlog: usize,
}
trait CallbackState {
    fn busy(&self) -> bool;
}
impl CallbackState for InputState {
    fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}
impl CallbackState for OutputState {
    fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// The box address remains stable from callback installation through disposal.
/// If CoreAudio cannot dispose an instance, deliberately retain its context; a
/// failed teardown must never turn a later callback into use-after-free.
struct HalUnit<T: CallbackState> {
    raw: Unit,
    state: Option<Box<T>>,
    initialized: bool,
    started: bool,
}
impl<T: CallbackState> HalUnit<T> {
    fn new(state: Box<T>) -> Result<Self, String> {
        let description = ComponentDescription {
            kind: fourcc(b"auou"),
            subtype: fourcc(b"ahal"),
            manufacturer: fourcc(b"appl"),
            flags: 0,
            mask: 0,
        };
        let component = unsafe { AudioComponentFindNext(ptr::null_mut(), &description) };
        if component.is_null() {
            return Err("macOS could not find its HAL audio unit.".into());
        }
        let mut raw = ptr::null_mut();
        check(
            unsafe { AudioComponentInstanceNew(component, &mut raw) },
            "Create HAL audio unit",
        )?;
        if raw.is_null() {
            return Err("macOS returned an empty HAL audio unit.".into());
        }
        Ok(Self {
            raw,
            state: Some(state),
            initialized: false,
            started: false,
        })
    }
    fn set<V>(
        &self,
        property: u32,
        scope: u32,
        bus: u32,
        value: &V,
        operation: &str,
    ) -> Result<(), String> {
        check(
            unsafe {
                AudioUnitSetProperty(
                    self.raw,
                    property,
                    scope,
                    bus,
                    (value as *const V).cast(),
                    size_of::<V>() as u32,
                )
            },
            operation,
        )
    }
    fn format(&self, scope: u32, bus: u32) -> Result<Format, String> {
        let mut format = Format::default();
        let mut bytes = size_of::<Format>() as u32;
        check(
            unsafe {
                AudioUnitGetProperty(
                    self.raw,
                    STREAM_FORMAT,
                    scope,
                    bus,
                    (&mut format as *mut Format).cast(),
                    &mut bytes,
                )
            },
            "Read HAL device format",
        )?;
        if !format.rate.is_finite()
            || !(8000.0..=384000.0).contains(&format.rate)
            || format.channels == 0
            || format.channels as usize > MAX_CHANNELS
        {
            return Err(
                "The selected audio device has an unsupported sample rate or channel count.".into(),
            );
        }
        Ok(format)
    }
    fn initialize(&mut self) -> Result<(), String> {
        check(
            unsafe { AudioUnitInitialize(self.raw) },
            "Initialize HAL audio unit",
        )?;
        self.initialized = true;
        Ok(())
    }
    fn start(&mut self) -> Result<(), String> {
        check(
            unsafe { AudioOutputUnitStart(self.raw) },
            "Start HAL audio unit",
        )?;
        self.started = true;
        Ok(())
    }
    fn context(&mut self) -> *mut c_void {
        ptr::from_mut(self.state.as_deref_mut().expect("unit owns callback state")).cast()
    }
}
impl<T: CallbackState> Drop for HalUnit<T> {
    fn drop(&mut self) {
        unsafe {
            // Attempt stop even after a failed start: some drivers partially start.
            AudioOutputUnitStop(self.raw);
            if self.initialized {
                AudioUnitUninitialize(self.raw);
            }
            let status = AudioComponentInstanceDispose(self.raw);
            if (status != 0 || self.state.as_ref().is_some_and(|state| state.busy()))
                && let Some(state) = self.state.take()
            {
                let _ = Box::leak(state);
            }
        }
    }
}

struct Route {
    shared: Arc<Shared>,
    input: Option<HalUnit<InputState>>,
    output: Option<HalUnit<OutputState>>,
    input_device: AudioDevice,
    output_device: AudioDevice,
    input_rate: f64,
    output_rate: f64,
    input_buffer: u32,
    output_buffer: u32,
    seconds_per_tick: f64,
    next_check: Instant,
}
impl Route {
    fn new(
        input: u32,
        output: u32,
        gain: f32,
        muted: bool,
        cancel: &AtomicBool,
    ) -> Result<(Self, f64), String> {
        permission::cancelled(cancel)?;
        let input_device = devices::validate(input, INPUT)?;
        let output_device = devices::validate(output, OUTPUT)?;
        // Hardware buffer properties affect every app and change asynchronously.
        // Read the existing sizes; this route changes only its own AUHAL state.
        let input_buffer = property::<u32>(input, fourcc(b"fsiz"), GLOBAL)?;
        let output_buffer = property::<u32>(output, fourcc(b"fsiz"), GLOBAL)?;
        if input_buffer == 0
            || output_buffer == 0
            || input_buffer as usize > CAPACITY
            || output_buffer as usize > CAPACITY
        {
            return Err("The selected device uses an unsupported audio buffer size. Choose another device or lower its buffer size.".into());
        }
        let input_rate = property::<f64>(input, fourcc(b"nsrt"), GLOBAL)?;
        let output_rate = property::<f64>(output, fourcc(b"nsrt"), GLOBAL)?;
        if !input_rate.is_finite()
            || !output_rate.is_finite()
            || !(8000.0..=384000.0).contains(&input_rate)
            || !(8000.0..=384000.0).contains(&output_rate)
        {
            return Err("The selected device has an unsupported sample rate.".into());
        }
        let ratio = input_rate / output_rate;
        let converter = Resampler::new(ratio);
        let taps = converter.taps();
        let target = priming_target(input_buffer, output_buffer, ratio, taps);
        let capacity = (target * 8).max(CAPACITY * 4).next_power_of_two();
        let max_backlog = (target + (input_buffer as usize * 2).max((input_rate * 0.015) as usize))
            .min(capacity - taps);
        let shared = Arc::new(Shared::new(capacity, gain, muted));
        let mut timebase = Timebase::default();
        if unsafe { mach_timebase_info(&mut timebase) } != 0 || timebase.denom == 0 {
            return Err("macOS could not provide its audio clock timebase.".into());
        }
        let seconds_per_tick = timebase.numer as f64 / timebase.denom as f64 / 1e9;
        let mut input_unit = HalUnit::new(Box::new(InputState {
            shared: shared.clone(),
            busy: AtomicBool::new(false),
            data: UnsafeCell::new(InputData {
                unit: ptr::null_mut(),
                scratch: Scratch::new(1),
                clock: Clock::new(input_rate, seconds_per_tick),
            }),
        }))?;
        input_unit.set(ENABLE_IO, AU_INPUT, 1, &1u32, "Enable microphone input")?;
        input_unit.set(
            ENABLE_IO,
            AU_OUTPUT,
            0,
            &0u32,
            "Disable microphone unit output",
        )?;
        input_unit.set(CURRENT_DEVICE, AU_GLOBAL, 0, &input, "Select microphone")?;
        let input_format = input_unit.format(AU_INPUT, 1)?;
        // A route change during setup must not silently use a stale conversion ratio.
        if (input_format.rate - input_rate).abs() > 0.5 {
            return Err("The microphone sample rate changed during setup. Try again.".into());
        }
        // One destination channel maps to physical channel 0. Fall back to
        // native capture and select channel 0 ourselves if mono mapping fails.
        let mono = input_unit
            .set(
                STREAM_FORMAT,
                AU_OUTPUT,
                1,
                &Format::planar(input_rate, 1),
                "Configure mono microphone format",
            )
            .and_then(|()| {
                input_unit.set(CHANNEL_MAP, AU_OUTPUT, 1, &[0i32], "Map microphone channel")
            })
            .is_ok();
        let input_channels = if mono {
            1
        } else {
            input_unit.set(
                STREAM_FORMAT,
                AU_OUTPUT,
                1,
                &Format::planar(input_rate, input_format.channels),
                "Configure native-rate microphone format",
            )?;
            input_format.channels as usize
        };
        let data = input_unit.state.as_deref_mut().unwrap().data.get_mut();
        data.unit = input_unit.raw;
        data.scratch = Scratch::new(input_channels);
        input_unit.set(
            ALLOCATE_BUFFER,
            AU_OUTPUT,
            1,
            &0u32,
            "Use preallocated microphone buffers",
        )?;
        // Maximum slice is storage capacity, not the hardware's requested latency.
        let _ = input_unit.set(
            MAX_FRAMES,
            AU_GLOBAL,
            0,
            &(CAPACITY as u32),
            "Set microphone callback capacity",
        );
        let input_context = input_unit.context();
        input_unit.set(
            INPUT_CALLBACK,
            AU_GLOBAL,
            0,
            &CallbackInfo {
                callback: input_callback,
                context: input_context,
            },
            "Install microphone callback",
        )?;

        let mut output_unit = HalUnit::new(Box::new(OutputState {
            shared: shared.clone(),
            busy: AtomicBool::new(false),
            data: UnsafeCell::new(OutputData {
                converter,
                clock: Clock::new(output_rate, seconds_per_tick),
                gain: 0.0,
            }),
            channels: 1,
            ratio,
            rate: output_rate,
            target,
            max_backlog,
        }))?;
        output_unit.set(ENABLE_IO, AU_OUTPUT, 0, &1u32, "Enable speaker output")?;
        output_unit.set(ENABLE_IO, AU_INPUT, 1, &0u32, "Disable speaker unit input")?;
        output_unit.set(CURRENT_DEVICE, AU_GLOBAL, 0, &output, "Select speaker")?;
        let output_format = output_unit.format(AU_OUTPUT, 0)?;
        if (output_format.rate - output_rate).abs() > 0.5 {
            return Err("The speaker sample rate changed during setup. Try again.".into());
        }
        let channels = output_format.channels as usize;
        let state = output_unit.state.as_deref_mut().unwrap();
        state.channels = channels;
        output_unit.set(
            STREAM_FORMAT,
            AU_INPUT,
            0,
            &Format::planar(output_rate, channels as u32),
            "Configure native-rate speaker format",
        )?;
        let _ = output_unit.set(
            MAX_FRAMES,
            AU_GLOBAL,
            0,
            &(CAPACITY as u32),
            "Set speaker callback capacity",
        );
        let output_context = output_unit.context();
        output_unit.set(
            RENDER_CALLBACK,
            AU_INPUT,
            0,
            &CallbackInfo {
                callback: output_callback,
                context: output_context,
            },
            "Install speaker callback",
        )?;
        input_unit.initialize()?;
        output_unit.initialize()?;
        // Another app may change shared buffer sizes while this route initializes.
        if property::<u32>(input, fourcc(b"fsiz"), GLOBAL)? != input_buffer
            || property::<u32>(output, fourcc(b"fsiz"), GLOBAL)? != output_buffer
        {
            return Err("The audio buffer size changed during setup. Try again.".into());
        }
        permission::cancelled(cancel)?;
        shared.enabled.store(true, Ordering::Release);
        input_unit.start()?;
        if let Err(error) = permission::cancelled(cancel) {
            shared.enabled.store(false, Ordering::Release);
            return Err(error);
        }
        output_unit.start()?;
        if let Err(error) = permission::cancelled(cancel) {
            shared.enabled.store(false, Ordering::Release);
            return Err(error);
        }
        let latency = 1000.0
            * ((devices::device_latency(input, INPUT) as f64
                + input_buffer as f64
                + target as f64
                + taps as f64 / 2.0)
                / input_rate
                + (devices::device_latency(output, OUTPUT) as f64 + output_buffer as f64)
                    / output_rate);
        Ok((
            Self {
                shared,
                input: Some(input_unit),
                output: Some(output_unit),
                input_device,
                output_device,
                input_rate,
                output_rate,
                input_buffer,
                output_buffer,
                seconds_per_tick,
                next_check: Instant::now(),
            },
            latency,
        ))
    }
    fn error(&mut self) -> Option<String> {
        let error = self.shared.error.swap(0, Ordering::Relaxed);
        if error != 0 {
            let status = error as u32 as i32;
            let operation = match (error >> 32) as u32 {
                ERROR_INPUT_RENDER => "Capture microphone audio",
                ERROR_INPUT_CAPACITY => return Some("The microphone requested more audio than its preallocated buffer can hold. Restart playback or choose another device.".into()),
                ERROR_OUTPUT_CAPACITY => return Some("The speaker requested more audio than its preallocated buffer can hold. Restart playback or choose another device.".into()),
                ERROR_INPUT_FORMAT => return Some("The microphone changed its audio buffer format. Restart playback.".into()),
                ERROR_OUTPUT_FORMAT => return Some("The speaker changed its audio buffer format. Restart playback.".into()),
                ERROR_REENTRANT => return Some("The audio driver invoked overlapping callbacks. Restart playback or choose another device.".into()),
                _ => "Run audio callback",
            };
            return Some(
                check(status, operation)
                    .err()
                    .unwrap_or_else(|| format!("{operation} failed.")),
            );
        }
        if Instant::now() < self.next_check {
            return None;
        }
        self.next_check = Instant::now() + Duration::from_millis(500);
        for (device, scope, rate, buffer) in [
            (
                &self.input_device,
                INPUT,
                self.input_rate,
                self.input_buffer,
            ),
            (
                &self.output_device,
                OUTPUT,
                self.output_rate,
                self.output_buffer,
            ),
        ] {
            if property::<u32>(device.id, fourcc(b"livn"), GLOBAL).unwrap_or(0) == 0 {
                return Some(format!(
                    "{} disconnected. Reconnect it or choose another device.",
                    device.name
                ));
            }
            let current_rate = property::<f64>(device.id, fourcc(b"nsrt"), GLOBAL).ok();
            if current_rate
                .is_none_or(|current| !current.is_finite() || (current - rate).abs() > 0.5)
                || property::<u32>(device.id, fourcc(b"fsiz"), GLOBAL).ok() != Some(buffer)
            {
                return Some(format!(
                    "{} changed its audio configuration. Restart playback.",
                    device.name
                ));
            }
            if devices::channel_count(device.id, scope).unwrap_or(0) == 0 {
                return Some(format!(
                    "{} is no longer available for this audio route.",
                    device.name
                ));
            }
        }
        let now = unsafe { mach_absolute_time() };
        for (last, name) in [
            (&self.shared.last_input, &self.input_device.name),
            (&self.shared.last_output, &self.output_device.name),
        ] {
            let last = last.load(Ordering::Relaxed);
            if now.saturating_sub(last) as f64 * self.seconds_per_tick > 5.0 {
                return Some(format!(
                    "{name} stopped delivering audio. Reconnect it or restart playback."
                ));
            }
        }
        None
    }
}
impl Drop for Route {
    fn drop(&mut self) {
        self.shared.enabled.store(false, Ordering::Release);
        drop(self.output.take());
        drop(self.input.take());
    }
}

pub struct AudioEngine {
    route: Option<Route>,
    gain: f32,
    muted: bool,
    last_diagnostics: AudioDiagnostics,
}
impl Default for AudioEngine {
    fn default() -> Self {
        Self::new()
    }
}
impl AudioEngine {
    pub fn new() -> Self {
        Self {
            route: None,
            gain: 1.0,
            muted: false,
            last_diagnostics: AudioDiagnostics::default(),
        }
    }
    pub fn start(
        &mut self,
        input: u32,
        output: u32,
        gain: f32,
        muted: bool,
    ) -> Result<f64, String> {
        self.start_cancellable(input, output, gain, muted, &AtomicBool::new(false))
    }
    pub fn start_cancellable(
        &mut self,
        input: u32,
        output: u32,
        gain: f32,
        muted: bool,
        cancel: &AtomicBool,
    ) -> Result<f64, String> {
        self.stop();
        self.gain = clean_gain(gain);
        self.muted = muted;
        permission::request(cancel)?;
        let (route, latency) = Route::new(input, output, self.gain, muted, cancel)?;
        self.last_diagnostics = AudioDiagnostics::default();
        self.route = Some(route);
        Ok(latency)
    }
    pub fn stop(&mut self) {
        if let Some(route) = self.route.take() {
            self.last_diagnostics = route.shared.diagnostics();
            drop(route);
        }
    }
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = clean_gain(gain);
        if let Some(route) = &self.route {
            route.shared.publish_gain(self.gain, self.muted);
        }
    }
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if let Some(route) = &self.route {
            route.shared.publish_gain(self.gain, self.muted);
        }
    }
    pub fn poll_error(&mut self) -> Option<String> {
        let error = self.route.as_mut().and_then(Route::error);
        if error.is_some() {
            self.stop();
        }
        error
    }
    pub fn diagnostics(&self) -> AudioDiagnostics {
        self.route
            .as_ref()
            .map(|route| route.shared.diagnostics())
            .unwrap_or(self.last_diagnostics)
    }
}
impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

// CoreAudio owns these invocations. Only this input unit touches InputData, only
// this output unit touches OutputData; reentrancy guards enforce that invariant.
unsafe extern "C" fn input_callback(
    context: *mut c_void,
    flags: *mut u32,
    timestamp: *const Timestamp,
    _: u32,
    frames: u32,
    _: *mut BufferList,
) -> Status {
    unsafe {
        let state = &*context.cast::<InputState>();
        let shared = &state.shared;
        shared.input_callbacks.fetch_add(1, Ordering::Relaxed);
        shared
            .last_input
            .store(mach_absolute_time(), Ordering::Relaxed);
        if !shared.enabled.load(Ordering::Acquire) || frames == 0 {
            return 0;
        }
        if state.busy.swap(true, Ordering::Acquire) {
            shared.fail(ERROR_REENTRANT, 0);
            return 0;
        }
        let _busy = Busy(&state.busy);
        if frames as usize > CAPACITY {
            shared.fail(ERROR_INPUT_CAPACITY, 0);
            return 0;
        }
        if timestamp.is_null() {
            shared.fail(ERROR_INPUT_FORMAT, 0);
            return 0;
        }
        let data = &mut *state.data.get();
        if data.clock.update(&*timestamp) {
            shared.reset.store(true, Ordering::Release);
        }
        shared
            .input_clock
            .store(data.clock.estimate.to_bits(), Ordering::Relaxed);
        let list = data.scratch.prepare(frames as usize);
        let status = AudioUnitRender(data.unit, flags, timestamp, 1, frames, list);
        if status != 0 {
            shared.fail(ERROR_INPUT_RENDER, status);
            return 0;
        }
        if !data.scratch.valid(frames as usize) {
            shared.fail(ERROR_INPUT_FORMAT, 0);
            return 0;
        }
        shared
            .input_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
        if !shared
            .ring
            .write_with(frames as usize, |index| data.scratch.mono_frame(index))
        {
            shared.overruns.fetch_add(1, Ordering::Relaxed);
            shared.reset.store(true, Ordering::Release);
        }
        0
    }
}

unsafe fn clear_output(list: *mut BufferList) -> bool {
    unsafe {
        if list.is_null() || (*list).count as usize > MAX_CHANNELS {
            return false;
        }
        let buffers = ptr::addr_of_mut!((*list).buffers).cast::<Buffer>();
        for index in 0..(*list).count as usize {
            let buffer = &*buffers.add(index);
            // Trust the HAL-provided byte capacity, never enlarge it ourselves.
            if !buffer.data.is_null() {
                ptr::write_bytes(buffer.data.cast::<u8>(), 0, buffer.bytes as usize);
            }
        }
        true
    }
}
unsafe extern "C" fn output_callback(
    context: *mut c_void,
    flags: *mut u32,
    timestamp: *const Timestamp,
    _: u32,
    frames: u32,
    list: *mut BufferList,
) -> Status {
    unsafe {
        let state = &*context.cast::<OutputState>();
        let shared = &state.shared;
        shared.output_callbacks.fetch_add(1, Ordering::Relaxed);
        shared
            .last_output
            .store(mach_absolute_time(), Ordering::Relaxed);
        if !clear_output(list) {
            shared.fail(ERROR_OUTPUT_FORMAT, 0);
            return 0;
        }
        if !flags.is_null() {
            *flags |= 1 << 4;
        }
        if !shared.enabled.load(Ordering::Acquire) || frames == 0 {
            return 0;
        }
        if state.busy.swap(true, Ordering::Acquire) {
            shared.fail(ERROR_REENTRANT, 0);
            return 0;
        }
        let _busy = Busy(&state.busy);
        if frames as usize > CAPACITY {
            shared.fail(ERROR_OUTPUT_CAPACITY, 0);
            return 0;
        }
        if timestamp.is_null() || (*list).count as usize != state.channels {
            shared.fail(ERROR_OUTPUT_FORMAT, 0);
            return 0;
        }
        let buffers = ptr::addr_of_mut!((*list).buffers).cast::<Buffer>();
        for channel in 0..state.channels {
            let buffer = &*buffers.add(channel);
            if buffer.channels != 1 || buffer.bytes < frames * 4 || buffer.data.is_null() {
                shared.fail(ERROR_OUTPUT_FORMAT, 0);
                return 0;
            }
        }
        let data = &mut *state.data.get();
        let discontinuity = data.clock.update(&*timestamp);
        let mut reader = shared.ring.reader();
        let taps = data.converter.taps();
        // Account for a larger current slice before backlog trimming. A driver
        // can increase slice size while its nominal buffer property is unchanged.
        let required =
            ((frames as f64 * state.ratio * 1.03).ceil() as usize + taps).max(state.target);
        if required + taps > shared.ring.capacity() {
            shared.fail(ERROR_OUTPUT_CAPACITY, 0);
            return 0;
        }
        let max_backlog = state
            .max_backlog
            .max(required + taps)
            .min(shared.ring.capacity());
        let reset = shared.reset.swap(false, Ordering::AcqRel);
        if reset || discontinuity || reader.available() > max_backlog {
            reader.keep_newest(required);
            data.converter.reset();
            shared.backlog_resets.fetch_add(1, Ordering::Relaxed);
        }
        // Prime enough for one complete output slice plus the filter lookahead.
        if !data.converter.playing {
            if reader.available() < required {
                return 0;
            }
            reader.keep_newest(required);
            data.converter.playing = true;
        }
        let clock_ratio =
            f64::from_bits(shared.input_clock.load(Ordering::Relaxed)) / data.clock.estimate;
        let ratio = data.converter.corrected_ratio(
            state.ratio,
            clock_ratio,
            reader.available(),
            state.target.max(required),
            frames as f64 / state.rate,
        );
        let target_gain = f32::from_bits(shared.target_gain.load(Ordering::Relaxed));
        if target_gain == 0.0 {
            data.gain = 0.0;
        }
        let gain_step = (target_gain - data.gain) / (frames as f32).min(128.0);
        let mut rendered = 0;
        for frame in 0..frames as usize {
            let Some(sample) = data.converter.next(&mut reader, ratio) else {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
                data.converter.reset();
                reader.consume(reader.available());
                break;
            };
            if frame < 128 {
                data.gain += gain_step;
            } else {
                data.gain = target_gain;
            }
            let sample = (sample.left * data.gain).clamp(-1.0, 1.0);
            for channel in 0..state.channels {
                let output = (*buffers.add(channel)).data.cast::<f32>();
                *output.add(frame) = sample;
            }
            rendered += 1;
        }
        shared.output_frames.fetch_add(rendered, Ordering::Relaxed);
        if rendered != 0 && target_gain != 0.0 && !flags.is_null() {
            *flags &= !(1 << 4);
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    // Exercise worker controls without opening devices or requesting permission.
    fn control_engine(shared: Arc<Shared>, gain: f32, muted: bool) -> AudioEngine {
        let device = AudioDevice {
            id: 0,
            name: "Test device".into(),
            is_bluetooth: false,
            is_built_in: false,
            is_continuity: false,
        };
        AudioEngine {
            route: Some(Route {
                shared,
                input: None,
                output: None,
                input_device: device.clone(),
                output_device: device,
                input_rate: 48000.0,
                output_rate: 48000.0,
                input_buffer: 128,
                output_buffer: 128,
                seconds_per_tick: 1e-9,
                next_check: Instant::now(),
            }),
            gain,
            muted,
            last_diagnostics: AudioDiagnostics::default(),
        }
    }

    fn scheduled_output(
        buffers: (u32, u32),
        rates: (f64, f64),
        drift: f64,
        jitter: f64,
        reduced_target: bool,
    ) -> AudioDiagnostics {
        let (input_buffer, output_buffer) = buffers;
        let (input_rate, output_rate) = rates;
        let ratio = input_rate / output_rate;
        let converter = Resampler::new(ratio);
        let taps = converter.taps();
        let target = if reduced_target {
            (input_buffer as usize).max((output_buffer as f64 * ratio).ceil() as usize) + taps
        } else {
            priming_target(input_buffer, output_buffer, ratio, taps)
        };
        let shared = Arc::new(Shared::new(65536, 1.0, false));
        shared.enabled.store(true, Ordering::Release);
        // Give the converter exact clock knowledge; failures cannot be blamed
        // on the slower host/sample clock estimator converging after startup.
        shared
            .input_clock
            .store((1.0 + drift).to_bits(), Ordering::Relaxed);
        let state = OutputState {
            shared: shared.clone(),
            busy: AtomicBool::new(false),
            data: UnsafeCell::new(OutputData {
                converter,
                clock: Clock::new(output_rate, 1e-9),
                gain: 0.0,
            }),
            channels: 1,
            ratio,
            rate: output_rate,
            target,
            max_backlog: target + (input_buffer as usize * 2).max((input_rate * 0.015) as usize),
        };
        let input_period = input_buffer as f64 / (input_rate * (1.0 + drift));
        let output_period = output_buffer as f64 / output_rate;
        let mut events = Vec::new();
        for index in 0..(0.25 / input_period).ceil() as usize {
            let delay = if matches!(index % 7, 3 | 4) {
                jitter
            } else {
                0.0
            };
            events.push((index as f64 * input_period + delay, false, index));
        }
        for index in 0..(0.25 / output_period).ceil() as usize {
            events.push((index as f64 * output_period, true, index));
        }
        events.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut samples = vec![0.0f32; output_buffer as usize];
        let mut list = BufferList {
            count: 1,
            buffers: [Buffer {
                channels: 1,
                bytes: output_buffer * 4,
                data: samples.as_mut_ptr().cast(),
            }],
        };
        for (seconds, output, index) in events {
            if output {
                let time = Timestamp {
                    sample: index as f64 * output_buffer as f64,
                    host: (seconds * 1e9).round() as u64 + 1,
                    flags: 3,
                    ..Timestamp::default()
                };
                let mut flags = 0;
                unsafe {
                    output_callback(
                        (&state as *const OutputState).cast_mut().cast(),
                        &mut flags,
                        &time,
                        0,
                        output_buffer,
                        &mut list,
                    );
                }
            } else {
                unsafe {
                    assert!(shared.ring.write_with(input_buffer as usize, |_| Frame {
                        left: 0.5,
                        right: 0.5,
                    }));
                }
            }
        }
        assert_eq!(shared.error.load(Ordering::Relaxed), 0);
        shared.diagnostics()
    }

    #[test]
    fn bridge_priming_keeps_phase_cushion_for_mismatched_clocks_and_callback_jitter() {
        for (buffers, rates, drift, jitter) in [
            ((128, 128), (48000.0, 48000.0), 0.0, 0.0001),
            ((128, 512), (48000.0, 48000.0), 0.0, 0.0001),
            ((128, 128), (44100.0, 48000.0), 0.0002, 0.0001),
            ((128, 256), (48000.0, 44100.0), -0.0002, 0.0001),
            ((128, 256), (16000.0, 48000.0), -0.0002, 0.0001),
        ] {
            let current = scheduled_output(buffers, rates, drift, jitter, false);
            let reduced = scheduled_output(buffers, rates, drift, jitter, true);
            assert_eq!(
                current.underruns, 0,
                "{buffers:?} frames, {rates:?} Hz, drift {drift}, jitter {jitter}"
            );
            assert!(
                reduced.underruns > 0,
                "max-only target unexpectedly survived: {buffers:?} frames, {rates:?} Hz"
            );
        }
    }

    #[test]
    fn silent_control_transitions_publish_only_zero_during_concurrent_reads() {
        let shared = Arc::new(Shared::new(256, 1.5, true));
        let start = Barrier::new(2);
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| {
                let mut engine = control_engine(shared.clone(), 1.5, true);
                start.wait();
                // Every complete control state is silent, including while unmuted.
                for _ in 0..100_000 {
                    engine.set_gain(0.0);
                    engine.set_muted(false);
                    engine.set_muted(true);
                    engine.set_gain(1.5);
                }
            });
            start.wait();
            for _ in 0..100_000 {
                assert_eq!(
                    f32::from_bits(shared.target_gain.load(Ordering::Relaxed)),
                    0.0
                );
            }
            writer.join().unwrap();
        });
        let mut engine = control_engine(shared.clone(), 1.5, true);
        engine.set_muted(false);
        assert_eq!(
            f32::from_bits(shared.target_gain.load(Ordering::Relaxed)),
            1.5
        );
    }

    #[test]
    fn host_sample_clock_tracks_drift_and_resets_discontinuities() {
        let mut clock = Clock::new(48000.0, 1e-9);
        for i in 0..400 {
            let time = Timestamp {
                sample: i as f64 * 480.048,
                host: 1 + i * 10_000_000,
                flags: 3,
                ..Timestamp::default()
            };
            assert!(!clock.update(&time));
        }
        assert!((clock.estimate - 1.0001).abs() < 0.00004);
        assert!(clock.update(&Timestamp {
            sample: 0.0,
            host: 5_000_000_000,
            flags: 3,
            ..Timestamp::default()
        }));
        assert_eq!(clock.estimate, 1.0);
    }
    #[test]
    fn sdk_layouts_match_64_bit_coreaudio() {
        assert_eq!(size_of::<Format>(), 40);
        assert_eq!(size_of::<Timestamp>(), 64);
        assert_eq!(size_of::<Buffer>(), 16);
        assert_eq!(offset_of!(BufferList, buffers), 8);
    }
    #[test]
    fn preallocated_input_list_has_full_capacity_per_channel() {
        let mut scratch = Scratch::new(2);
        scratch.prepare(CAPACITY);
        assert!(scratch.valid(CAPACITY));
        scratch.prepare(128);
        assert!(scratch.valid(128));
    }
    #[test]
    fn larger_mono_output_slice_fills_every_channel_and_mutes_without_touching_guards() {
        // Asymmetric native stereo must monitor only channel 0, duplicated as mono.
        let mut scratch = Scratch::new(2);
        scratch.samples[..8000].fill(0.5);
        scratch.samples[CAPACITY..CAPACITY + 8000].fill(-0.25);
        let source = scratch.mono_frame(0);
        assert_eq!((source.left, source.right), (0.5, 0.5));
        #[repr(C)]
        struct OutputList {
            count: u32,
            buffers: [Buffer; 4],
        }

        for muted in [false, true] {
            let shared = Arc::new(Shared::new(65536, 1.0, false));
            let mut engine = control_engine(shared.clone(), 1.0, false);
            engine.set_muted(muted);
            shared.enabled.store(true, Ordering::Release);
            unsafe {
                assert!(shared.ring.write_with(8000, |i| scratch.mono_frame(i)));
            }
            let state = OutputState {
                shared: shared.clone(),
                busy: AtomicBool::new(false),
                data: UnsafeCell::new(OutputData {
                    converter: Resampler::new(1.0),
                    clock: Clock::new(48000.0, 1e-9),
                    // Mute must silence even a previously positive smoothing value.
                    gain: 1.0,
                }),
                channels: 4,
                ratio: 1.0,
                rate: 48000.0,
                target: 288,
                max_backlog: 1000,
            };
            let mut samples: [Vec<f32>; 4] = std::array::from_fn(|_| vec![42.0; 4098]);
            let mut list = OutputList {
                count: 4,
                buffers: samples.each_mut().map(|channel| Buffer {
                    channels: 1,
                    bytes: 4096 * 4,
                    data: unsafe { channel.as_mut_ptr().add(1).cast() },
                }),
            };
            let mut flags = 0;
            let time = Timestamp {
                sample: 0.0,
                host: 1,
                flags: 3,
                ..Timestamp::default()
            };
            unsafe {
                output_callback(
                    (&state as *const OutputState).cast_mut().cast(),
                    &mut flags,
                    &time,
                    0,
                    4096,
                    (&mut list as *mut OutputList).cast(),
                );
            }
            assert_eq!(shared.error.load(Ordering::Relaxed), 0);
            assert_eq!(shared.output_frames.load(Ordering::Relaxed), 4096);
            assert_eq!(flags & (1 << 4) != 0, muted);
            for channel in &samples {
                assert_eq!((channel[0], channel[4097]), (42.0, 42.0));
                assert_eq!(channel[1..4097], samples[0][1..4097]);
                if muted {
                    assert!(channel[1..4097].iter().all(|sample| *sample == 0.0));
                } else {
                    assert!((channel[4096] - 0.5).abs() < 1e-5);
                }
            }
        }
    }
}
