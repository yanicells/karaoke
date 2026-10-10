use super::ffi::*;
use super::{AudioDevice, Devices};
use std::ffi::CStr;
use std::mem::{offset_of, size_of};
use std::ptr;

pub fn check(status: Status, operation: &str) -> Result<(), String> {
    if status == 0 {
        return Ok(());
    }
    let bytes = (status as u32).to_be_bytes();
    let code = if bytes.iter().all(|byte| (32..127).contains(byte)) {
        format!("'{}' ({status})", String::from_utf8_lossy(&bytes))
    } else {
        status.to_string()
    };
    Err(format!("{operation} failed (CoreAudio {code})."))
}
pub fn property<T: Default>(id: u32, selector: u32, scope: u32) -> Result<T, String> {
    let mut value = T::default();
    let mut bytes = size_of::<T>() as u32;
    // Only SDK POD types are used with this private helper.
    let status = unsafe {
        AudioObjectGetPropertyData(
            id,
            &Address::new(selector, scope),
            0,
            ptr::null(),
            &mut bytes,
            (&mut value as *mut T).cast(),
        )
    };
    check(status, "Read audio device property")?;
    if bytes as usize != size_of::<T>() {
        return Err("The audio device returned an invalid property size.".into());
    }
    Ok(value)
}
pub fn vector<T: Default + Copy>(id: u32, selector: u32, scope: u32) -> Result<Vec<T>, String> {
    let address = Address::new(selector, scope);
    for _ in 0..3 {
        let mut bytes = 0;
        check(
            unsafe { AudioObjectGetPropertyDataSize(id, &address, 0, ptr::null(), &mut bytes) },
            "Read audio device list size",
        )?;
        if !(bytes as usize).is_multiple_of(size_of::<T>()) || bytes > 1024 * 1024 {
            return Err("CoreAudio returned an invalid device list.".into());
        }
        if bytes == 0 {
            return Ok(Vec::new());
        }
        let mut values = vec![T::default(); bytes as usize / size_of::<T>()];
        let capacity_bytes = bytes;
        let status = unsafe {
            AudioObjectGetPropertyData(
                id,
                &address,
                0,
                ptr::null(),
                &mut bytes,
                values.as_mut_ptr().cast(),
            )
        };
        if status != 0 {
            continue;
        }
        if bytes > capacity_bytes || !(bytes as usize).is_multiple_of(size_of::<T>()) {
            continue;
        }
        values.truncate(bytes as usize / size_of::<T>());
        return Ok(values);
    }
    Err("The audio device list changed while it was being read. Try again.".into())
}
pub fn channel_count(id: u32, scope: u32) -> Result<u32, String> {
    let address = Address::new(fourcc(b"slay"), scope);
    let mut bytes = 0;
    check(
        unsafe { AudioObjectGetPropertyDataSize(id, &address, 0, ptr::null(), &mut bytes) },
        "Read audio channel configuration size",
    )?;
    let offset = offset_of!(BufferList, buffers);
    if (bytes as usize) < offset || bytes > 1024 * 1024 {
        return Err("The audio device has an invalid channel configuration.".into());
    }
    // u64 storage provides the alignment required by AudioBufferList on macOS.
    let mut memory = vec![0u64; (bytes as usize).div_ceil(8)];
    let allocated_bytes = bytes;
    check(
        unsafe {
            AudioObjectGetPropertyData(
                id,
                &address,
                0,
                ptr::null(),
                &mut bytes,
                memory.as_mut_ptr().cast(),
            )
        },
        "Read audio channel configuration",
    )?;
    if bytes > allocated_bytes {
        return Err("The audio device channel configuration changed. Try again.".into());
    }
    let list = memory.as_ptr().cast::<BufferList>();
    let count = unsafe { (*list).count } as usize;
    if offset + count * size_of::<Buffer>() > bytes as usize {
        return Err("The audio device returned an incomplete channel configuration.".into());
    }
    let buffers = unsafe {
        std::slice::from_raw_parts(ptr::addr_of!((*list).buffers).cast::<Buffer>(), count)
    };
    Ok(buffers.iter().map(|buffer| buffer.channels).sum())
}
fn name(id: u32) -> Result<String, String> {
    let mut value = ptr::null::<std::ffi::c_void>();
    let mut bytes = size_of_val(&value) as u32;
    check(
        unsafe {
            AudioObjectGetPropertyData(
                id,
                &Address::new(fourcc(b"lnam"), GLOBAL),
                0,
                ptr::null(),
                &mut bytes,
                (&mut value as *mut *const std::ffi::c_void).cast(),
            )
        },
        "Read audio device name",
    )?;
    if value.is_null() {
        return Ok(format!("Audio device {id}"));
    }
    // The CoreAudio name property returns a retained CFString (SDK contract).
    let result = unsafe {
        let length = CFStringGetLength(value);
        let max = CFStringGetMaximumSizeForEncoding(length, 0x08000100);
        if !(0..=1024 * 1024).contains(&max) {
            Err("The audio device name is invalid.".into())
        } else {
            let mut buffer = vec![0u8; max as usize + 1];
            if CFStringGetCString(
                value,
                buffer.as_mut_ptr().cast(),
                buffer.len() as isize,
                0x08000100,
            ) != 0
            {
                Ok(CStr::from_ptr(buffer.as_ptr().cast())
                    .to_string_lossy()
                    .into_owned())
            } else {
                Err("The audio device name could not be decoded.".into())
            }
        }
    };
    unsafe {
        CFRelease(value);
    }
    result
}
pub fn describe(id: u32) -> Result<AudioDevice, String> {
    let transport: u32 = property(id, fourcc(b"tran"), GLOBAL).unwrap_or(0);
    Ok(AudioDevice {
        id,
        name: name(id)?,
        is_bluetooth: transport == fourcc(b"blue") || transport == fourcc(b"blea"),
        is_built_in: transport == fourcc(b"bltn"),
        is_continuity: [fourcc(b"ccap"), fourcc(b"ccwd"), fourcc(b"ccwl")].contains(&transport),
    })
}
pub fn devices() -> Result<Devices, String> {
    let ids: Vec<u32> = vector(1, fourcc(b"dev#"), GLOBAL)?;
    let mut result = Devices {
        inputs: Vec::new(),
        outputs: Vec::new(),
    };
    for id in ids {
        // An unplugged device can disappear between enumeration and metadata reads.
        let Ok(device) = describe(id) else {
            continue;
        };
        if channel_count(id, INPUT).unwrap_or(0) > 0 {
            result.inputs.push(device.clone());
        }
        if channel_count(id, OUTPUT).unwrap_or(0) > 0 {
            result.outputs.push(device);
        }
    }
    Ok(result)
}
pub fn validate(id: u32, scope: u32) -> Result<AudioDevice, String> {
    let direction = if scope == INPUT {
        "microphone"
    } else {
        "speaker"
    };
    let device = describe(id).map_err(|_| {
        format!("The selected {direction} is no longer available. Choose another device.")
    })?;
    if property::<u32>(id, fourcc(b"livn"), GLOBAL).unwrap_or(0) == 0
        || channel_count(id, scope)? == 0
    {
        return Err(format!(
            "{} is not available as a {direction}. Reconnect it or choose another device.",
            device.name
        ));
    }
    Ok(device)
}
pub fn device_latency(id: u32, scope: u32) -> u32 {
    let device = property::<u32>(id, fourcc(b"ltnc"), scope).unwrap_or(0);
    let safety = property::<u32>(id, fourcc(b"saft"), scope).unwrap_or(0);
    let streams: Vec<u32> = vector(id, fourcc(b"stm#"), scope).unwrap_or_default();
    let stream = streams
        .into_iter()
        .map(|stream| property::<u32>(stream, fourcc(b"ltnc"), GLOBAL).unwrap_or(0))
        .max()
        .unwrap_or(0);
    device.saturating_add(safety).saturating_add(stream)
}
