//! Small, SDK-defined ABI surface. All framework calls happen on the audio worker,
//! except AudioUnitRender in the input callback and mach_absolute_time.
#![allow(non_snake_case)]

use std::ffi::{c_char, c_void};

pub type Unit = *mut c_void;
pub type Status = i32;
pub const fn fourcc(bytes: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*bytes)
}
pub const GLOBAL: u32 = fourcc(b"glob");
pub const INPUT: u32 = fourcc(b"inpt");
pub const OUTPUT: u32 = fourcc(b"outp");
pub const AU_GLOBAL: u32 = 0;
pub const AU_INPUT: u32 = 1;
pub const AU_OUTPUT: u32 = 2;
pub const STREAM_FORMAT: u32 = 8;
pub const MAX_FRAMES: u32 = 14;
pub const RENDER_CALLBACK: u32 = 23;
pub const ALLOCATE_BUFFER: u32 = 51;
pub const CURRENT_DEVICE: u32 = 2000;
pub const CHANNEL_MAP: u32 = 2002;
pub const ENABLE_IO: u32 = 2003;
pub const INPUT_CALLBACK: u32 = 2005;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Address {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}
impl Address {
    pub fn new(selector: u32, scope: u32) -> Self {
        Self {
            selector,
            scope,
            element: 0,
        }
    }
}

#[repr(C)]
pub struct ComponentDescription {
    pub kind: u32,
    pub subtype: u32,
    pub manufacturer: u32,
    pub flags: u32,
    pub mask: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Format {
    pub rate: f64,
    pub format: u32,
    pub flags: u32,
    pub bytes_per_packet: u32,
    pub frames_per_packet: u32,
    pub bytes_per_frame: u32,
    pub channels: u32,
    pub bits: u32,
    pub reserved: u32,
}
impl Format {
    pub fn planar(rate: f64, channels: u32) -> Self {
        Self {
            rate,
            format: fourcc(b"lpcm"),
            flags: 1 | 8 | 32,
            bytes_per_packet: 4,
            frames_per_packet: 1,
            bytes_per_frame: 4,
            channels,
            bits: 32,
            reserved: 0,
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct SmpteTime {
    pub subframes: i16,
    pub divisor: i16,
    pub counter: u32,
    pub kind: u32,
    pub flags: u32,
    pub hours: i16,
    pub minutes: i16,
    pub seconds: i16,
    pub frames: i16,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Timestamp {
    pub sample: f64,
    pub host: u64,
    pub scalar: f64,
    pub word_clock: u64,
    pub smpte: SmpteTime,
    pub flags: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Buffer {
    pub channels: u32,
    pub bytes: u32,
    pub data: *mut c_void,
}
#[repr(C)]
pub struct BufferList {
    pub count: u32,
    pub buffers: [Buffer; 1],
}
pub type Callback = unsafe extern "C" fn(
    *mut c_void,
    *mut u32,
    *const Timestamp,
    u32,
    u32,
    *mut BufferList,
) -> Status;
#[repr(C)]
pub struct CallbackInfo {
    pub callback: Callback,
    pub context: *mut c_void,
}
#[repr(C)]
#[derive(Default)]
pub struct Timebase {
    pub numer: u32,
    pub denom: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    pub fn AudioObjectGetPropertyDataSize(
        id: u32,
        address: *const Address,
        qualifier_bytes: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> Status;
    pub fn AudioObjectGetPropertyData(
        id: u32,
        address: *const Address,
        qualifier_bytes: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> Status;
}
#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    pub fn AudioComponentFindNext(
        previous: *mut c_void,
        desc: *const ComponentDescription,
    ) -> *mut c_void;
    pub fn AudioComponentInstanceNew(component: *mut c_void, unit: *mut Unit) -> Status;
    pub fn AudioComponentInstanceDispose(unit: Unit) -> Status;
    pub fn AudioUnitSetProperty(
        unit: Unit,
        property: u32,
        scope: u32,
        bus: u32,
        data: *const c_void,
        bytes: u32,
    ) -> Status;
    pub fn AudioUnitGetProperty(
        unit: Unit,
        property: u32,
        scope: u32,
        bus: u32,
        data: *mut c_void,
        bytes: *mut u32,
    ) -> Status;
    pub fn AudioUnitInitialize(unit: Unit) -> Status;
    pub fn AudioUnitUninitialize(unit: Unit) -> Status;
    pub fn AudioOutputUnitStart(unit: Unit) -> Status;
    pub fn AudioOutputUnitStop(unit: Unit) -> Status;
    pub fn AudioUnitRender(
        unit: Unit,
        flags: *mut u32,
        time: *const Timestamp,
        bus: u32,
        frames: u32,
        data: *mut BufferList,
    ) -> Status;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFRelease(value: *const c_void);
    pub fn CFStringGetLength(value: *const c_void) -> isize;
    pub fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    pub fn CFStringGetCString(
        value: *const c_void,
        buffer: *mut c_char,
        bytes: isize,
        encoding: u32,
    ) -> u8;
}
unsafe extern "C" {
    pub fn mach_absolute_time() -> u64;
    pub fn mach_timebase_info(info: *mut Timebase) -> i32;
}
