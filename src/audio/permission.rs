//! Native AVCaptureDevice authorization. The completion block owns an Arc through
//! Blocks ABI copy/dispose helpers, so cancellation never leaves a dangling capture.
use std::ffi::{c_char, c_void};
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::Duration;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: *mut c_void;
}
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}
unsafe extern "C" {
    static _NSConcreteStackBlock: c_void;
    fn _Block_copy(block: *const c_void) -> *mut c_void;
    fn _Block_release(block: *const c_void);
}
struct Pool(*mut c_void);
impl Pool {
    fn new() -> Self {
        Self(unsafe { objc_autoreleasePoolPush() })
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        unsafe {
            objc_autoreleasePoolPop(self.0);
        }
    }
}

unsafe fn class(name: &std::ffi::CStr) -> *mut c_void {
    unsafe { objc_getClass(name.as_ptr()) }
}
unsafe fn selector(name: &std::ffi::CStr) -> *mut c_void {
    unsafe { sel_registerName(name.as_ptr()) }
}
unsafe fn object0(object: *mut c_void, name: &std::ffi::CStr) -> *mut c_void {
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { send(object, selector(name)) }
}
unsafe fn object1(
    object: *mut c_void,
    name: &std::ffi::CStr,
    argument: *const c_void,
) -> *mut c_void {
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void) -> *mut c_void =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { send(object, selector(name), argument) }
}
fn status_code() -> isize {
    let _pool = Pool::new();
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> isize =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        send(
            class(c"AVCaptureDevice"),
            selector(c"authorizationStatusForMediaType:"),
            AVMediaTypeAudio,
        )
    }
}
pub fn permission_status() -> &'static str {
    match status_code() {
        0 => "not-determined",
        1 => "restricted",
        2 => "denied",
        3 => "authorized",
        _ => "unknown",
    }
}
#[repr(C)]
struct Descriptor {
    reserved: usize,
    size: usize,
    copy: unsafe extern "C" fn(*mut PermissionBlock, *const PermissionBlock),
    dispose: unsafe extern "C" fn(*mut PermissionBlock),
    signature: *const c_char,
}
// Immutable descriptor, static functions and static C-string; Blocks reads it only.
unsafe impl Sync for Descriptor {}
#[repr(C)]
struct PermissionBlock {
    isa: *const c_void,
    flags: i32,
    reserved: i32,
    invoke: unsafe extern "C" fn(*mut PermissionBlock, i8),
    descriptor: *const Descriptor,
    result: *const AtomicI32,
}
unsafe extern "C" fn copy(dst: *mut PermissionBlock, _: *const PermissionBlock) {
    unsafe {
        Arc::increment_strong_count((*dst).result);
    }
}
unsafe extern "C" fn dispose(block: *mut PermissionBlock) {
    unsafe {
        Arc::decrement_strong_count((*block).result);
    }
}
unsafe extern "C" fn completion(block: *mut PermissionBlock, granted: i8) {
    unsafe {
        (*(*block).result).store(if granted != 0 { 1 } else { 2 }, Ordering::Release);
    }
}
#[cfg(target_arch = "aarch64")]
const SIGNATURE: &std::ffi::CStr = c"v@?B";
#[cfg(not(target_arch = "aarch64"))]
const SIGNATURE: &std::ffi::CStr = c"v@?c";
static DESCRIPTOR: Descriptor = Descriptor {
    reserved: 0,
    size: size_of::<PermissionBlock>(),
    copy,
    dispose,
    signature: SIGNATURE.as_ptr(),
};

pub fn request(cancel: &AtomicBool) -> Result<(), String> {
    cancelled(cancel)?;
    match status_code() {
        3 => return Ok(()),
        1 => return Err("Microphone access is restricted on this Mac.".into()),
        2 => return Err("Microphone access is denied. Enable KaraokeMic in System Settings → Privacy & Security → Microphone, then reopen the app.".into()),
        0 => {},
        _ => return Err("macOS returned an unknown microphone permission status.".into()),
    }
    let _pool = Pool::new();
    // macOS aborts a process that requests microphone permission without this key.
    let description = unsafe {
        let bundle = object0(class(c"NSBundle"), c"mainBundle");
        let key = object1(
            class(c"NSString"),
            c"stringWithUTF8String:",
            c"NSMicrophoneUsageDescription".as_ptr().cast(),
        );
        object1(bundle, c"objectForInfoDictionaryKey:", key)
    };
    if description.is_null() {
        return Err("Microphone permission needs the packaged KaraokeMic.app. Launch the app bundle; its Info.plist must include NSMicrophoneUsageDescription.".into());
    }
    let result = Arc::new(AtomicI32::new(0));
    let block = PermissionBlock {
        isa: ptr::addr_of!(_NSConcreteStackBlock),
        flags: (1 << 25) | (1 << 30),
        reserved: 0,
        invoke: completion,
        descriptor: &DESCRIPTOR,
        result: Arc::as_ptr(&result),
    };
    let copied = unsafe { _Block_copy((&block as *const PermissionBlock).cast()) };
    if copied.is_null() {
        return Err("Could not prepare the microphone permission request.".into());
    }
    let send: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        send(
            class(c"AVCaptureDevice"),
            selector(c"requestAccessForMediaType:completionHandler:"),
            AVMediaTypeAudio,
            copied,
        );
        _Block_release(copied);
    }
    loop {
        cancelled(cancel)?;
        match result.load(Ordering::Acquire) {
            1 => return cancelled(cancel),
            2 => return Err("Microphone access was denied. Enable KaraokeMic in System Settings → Privacy & Security → Microphone.".into()),
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}
pub fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Audio startup was cancelled.".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_permission_block_keeps_result_alive_after_cancellation() {
        let result = Arc::new(AtomicI32::new(0));
        let weak = Arc::downgrade(&result);
        let block = PermissionBlock {
            isa: ptr::addr_of!(_NSConcreteStackBlock),
            flags: (1 << 25) | (1 << 30),
            reserved: 0,
            invoke: completion,
            descriptor: &DESCRIPTOR,
            result: Arc::as_ptr(&result),
        };
        let copied = unsafe { _Block_copy((&block as *const PermissionBlock).cast()) };
        assert!(!copied.is_null());
        drop(result);
        unsafe {
            completion(copied.cast(), 1);
        }
        assert_eq!(weak.upgrade().unwrap().load(Ordering::Acquire), 1);
        unsafe {
            _Block_release(copied);
        }
        assert!(weak.upgrade().is_none());
    }
}
