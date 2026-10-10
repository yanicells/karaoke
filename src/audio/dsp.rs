//! Bounded SPSC transport and a precomputed windowed-sinc converter. The producer
//! never advances the consumer cursor, even when full; this prevents overwrite races.
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const BASE_TAPS: usize = 64;
const PHASES: usize = 1024;

#[derive(Clone, Copy, Default, Debug)]
pub struct Frame {
    pub left: f32,
    pub right: f32,
}
#[repr(align(64))]
struct Cursor(AtomicUsize);
pub struct Ring {
    data: Box<[UnsafeCell<Frame>]>,
    mask: usize,
    read: Cursor,
    write: Cursor,
}
// One callback owns write/storage of unpublished slots; one owns read. Release /
// acquire publication prevents either side from touching the other's live slots.
unsafe impl Sync for Ring {}
impl Ring {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(2).next_power_of_two();
        Self {
            data: (0..capacity)
                .map(|_| UnsafeCell::new(Frame::default()))
                .collect(),
            mask: capacity - 1,
            read: Cursor(AtomicUsize::new(0)),
            write: Cursor(AtomicUsize::new(0)),
        }
    }
    pub fn capacity(&self) -> usize {
        self.data.len()
    }
    pub fn queued(&self) -> usize {
        let read = self.read.0.load(Ordering::Acquire);
        self.write
            .0
            .load(Ordering::Acquire)
            .wrapping_sub(read)
            .min(self.capacity())
    }
    /// Only the single input callback may call this. A full block is dropped if
    /// it cannot fit; no unread frame is ever overwritten.
    pub unsafe fn write_with(&self, frames: usize, mut sample: impl FnMut(usize) -> Frame) -> bool {
        let write = self.write.0.load(Ordering::Relaxed);
        let read = self.read.0.load(Ordering::Acquire);
        if frames > self.capacity().saturating_sub(write.wrapping_sub(read)) {
            return false;
        }
        for offset in 0..frames {
            unsafe {
                *self.data[write.wrapping_add(offset) & self.mask].get() = sample(offset);
            }
        }
        self.write
            .0
            .store(write.wrapping_add(frames), Ordering::Release);
        true
    }
    /// Only the single output callback may hold a reader.
    pub unsafe fn reader(&self) -> Reader<'_> {
        let read = self.read.0.load(Ordering::Relaxed);
        let write = self.write.0.load(Ordering::Acquire);
        Reader {
            ring: self,
            cursor: read,
            remaining: write.wrapping_sub(read).min(self.capacity()),
        }
    }
}
pub struct Reader<'a> {
    ring: &'a Ring,
    cursor: usize,
    remaining: usize,
}
impl Reader<'_> {
    pub fn available(&self) -> usize {
        self.remaining
    }
    pub fn peek(&self, offset: usize) -> Frame {
        debug_assert!(offset < self.remaining);
        unsafe { *self.ring.data[self.cursor.wrapping_add(offset) & self.ring.mask].get() }
    }
    pub fn consume(&mut self, frames: usize) {
        let frames = frames.min(self.remaining);
        self.cursor = self.cursor.wrapping_add(frames);
        self.remaining -= frames;
    }
    pub fn keep_newest(&mut self, frames: usize) {
        self.consume(self.remaining.saturating_sub(frames));
    }
}
impl Drop for Reader<'_> {
    fn drop(&mut self) {
        self.ring.read.0.store(self.cursor, Ordering::Release);
    }
}

pub struct Resampler {
    coefficients: Box<[f32]>,
    taps: usize,
    half: usize,
    position: f64,
    pub playing: bool,
    filtered_error: f64,
    integral: f64,
    ramp: f32,
}
impl Resampler {
    pub fn new(nominal_ratio: f64) -> Self {
        let taps = BASE_TAPS * nominal_ratio.max(1.0).ceil() as usize;
        let half = taps / 2;
        // Reserve a small guard band for clock/occupancy correction when downsampling.
        // Scale filter length with the downsampling factor: a fixed short filter
        // cannot suppress aliasing when bridging e.g. 48 kHz to a 16 kHz device.
        let cutoff = 0.90 / nominal_ratio.max(1.0) / 1.03;
        let mut coefficients = vec![0.0; taps * (PHASES + 1)].into_boxed_slice();
        for (phase, row) in coefficients.chunks_exact_mut(taps).enumerate() {
            let fraction = phase as f64 / PHASES as f64;
            let mut sum = 0.0;
            for (tap, weight) in row.iter_mut().enumerate() {
                let distance = tap as f64 - (half - 1) as f64 - fraction;
                let x = std::f64::consts::PI * distance * cutoff;
                let sinc = if x.abs() < 1e-12 { 1.0 } else { x.sin() / x };
                let angle = 2.0 * std::f64::consts::PI * tap as f64 / (taps - 1) as f64;
                let window = 0.42 - 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos();
                *weight = (cutoff * sinc * window) as f32;
                sum += *weight as f64;
            }
            for weight in row {
                *weight /= sum as f32;
            }
        }
        Self {
            coefficients,
            taps,
            half,
            position: (half - 1) as f64,
            playing: false,
            filtered_error: 0.0,
            integral: 0.0,
            ramp: 0.0,
        }
    }
    pub fn taps(&self) -> usize {
        self.taps
    }
    pub fn reset(&mut self) {
        self.position = (self.half - 1) as f64;
        self.playing = false;
        self.filtered_error = 0.0;
        self.integral = 0.0;
        self.ramp = 0.0;
    }
    /// Slow occupancy feedback covers clock metadata omissions and wireless jitter.
    /// Both the PI integrator and total correction are bounded to prevent pitch runaway.
    pub fn corrected_ratio(
        &mut self,
        nominal: f64,
        clock_ratio: f64,
        queued: usize,
        target: usize,
        seconds: f64,
    ) -> f64 {
        let error = queued as f64 - target as f64;
        let alpha = (seconds / 0.5).clamp(0.0, 1.0);
        self.filtered_error += alpha * (error - self.filtered_error);
        self.integral =
            (self.integral + self.filtered_error * seconds * 0.0000005).clamp(-0.003, 0.003);
        let correction = (self.filtered_error * 0.000002 + self.integral).clamp(-0.005, 0.005);
        nominal * clock_ratio.clamp(0.98, 1.02) * (1.0 + correction)
    }
    pub fn next(&mut self, reader: &mut Reader<'_>, ratio: f64) -> Option<Frame> {
        let whole = self.position.floor() as usize;
        let first = whole - (self.half - 1);
        if first + self.taps > reader.available() {
            return None;
        }
        let phase = ((self.position - whole as f64) * PHASES as f64).round() as usize;
        let start = phase.min(PHASES) * self.taps;
        let weights = &self.coefficients[start..start + self.taps];
        let mut frame = Frame::default();
        for (tap, weight) in weights.iter().enumerate() {
            let source = reader.peek(first + tap);
            frame.left += source.left * weight;
            frame.right += source.right * weight;
        }
        self.position += ratio;
        let consumed = (self.position.floor() as usize)
            .saturating_sub(self.half - 1)
            .min(reader.available());
        reader.consume(consumed);
        self.position -= consumed as f64;
        self.ramp = (self.ramp + 1.0 / 128.0).min(1.0);
        frame.left *= self.ramp;
        frame.right *= self.ramp;
        Some(frame)
    }
}

pub fn clean_gain(gain: f32) -> f32 {
    if gain.is_finite() {
        gain.clamp(0.0, 4.0)
    } else {
        1.0
    }
}
pub fn clean_sample(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn full_ring_preserves_unread_data_and_wraps() {
        let ring = Ring::new(8);
        unsafe {
            assert!(ring.write_with(8, |i| Frame {
                left: i as f32,
                right: -(i as f32)
            }));
            assert!(!ring.write_with(1, |_| Frame::default()));
            let mut reader = ring.reader();
            assert_eq!(reader.peek(7).left, 7.0);
            reader.consume(6);
        }
        unsafe {
            assert!(ring.write_with(6, |i| Frame {
                left: (i + 8) as f32,
                right: 0.0
            }));
            let reader = ring.reader();
            for i in 0..8 {
                assert_eq!(reader.peek(i).left, (i + 6) as f32);
            }
        }
    }
    #[test]
    fn spsc_publication_survives_concurrent_wraparound() {
        let ring = Arc::new(Ring::new(256));
        let writer = ring.clone();
        let producer = std::thread::spawn(move || {
            for i in 0..100_000 {
                while !unsafe {
                    writer.write_with(1, |_| Frame {
                        left: i as f32,
                        right: -(i as f32),
                    })
                } {
                    std::thread::yield_now();
                }
            }
        });
        for i in 0..100_000 {
            loop {
                let mut reader = unsafe { ring.reader() };
                if reader.available() != 0 {
                    let frame = reader.peek(0);
                    assert_eq!(frame.left, i as f32);
                    assert_eq!(frame.right, -(i as f32));
                    reader.consume(1);
                    break;
                }
                drop(reader);
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();
    }
    #[test]
    fn conversion_consumes_native_rate_and_preserves_stereo_dc() {
        for ratio in [44100.0 / 48000.0, 48000.0 / 44100.0, 3.0, 1.0 / 3.0] {
            let ring = Ring::new(8192);
            unsafe {
                assert!(ring.write_with(8000, |_| Frame {
                    left: 0.5,
                    right: -0.25
                }));
            }
            let mut reader = unsafe { ring.reader() };
            let mut resampler = Resampler::new(ratio);
            for i in 0..1000 {
                let frame = resampler.next(&mut reader, ratio).unwrap();
                if i > 128 {
                    assert!((frame.left - 0.5).abs() < 1e-5);
                    assert!((frame.right + 0.25).abs() < 1e-5);
                }
            }
            let consumed = 8000 - reader.available();
            assert!((consumed as f64 - 1000.0 * ratio).abs() <= 1.0);
        }
    }
    #[test]
    fn downsampling_rejects_alias_band() {
        let ring = Ring::new(8192);
        unsafe {
            ring.write_with(8000, |i| Frame {
                left: (i as f32 * std::f32::consts::PI * 0.8).sin(),
                right: 0.0,
            });
        }
        let mut reader = unsafe { ring.reader() };
        let mut converter = Resampler::new(3.0);
        let mut energy = 0.0;
        for i in 0..2000 {
            let v = converter.next(&mut reader, 3.0).unwrap().left;
            if i > 128 {
                energy += v * v;
            }
        }
        assert!(energy / 1871.0 < 0.0001);
    }
    #[test]
    fn alias_rejection_scales_with_the_conversion_factor() {
        for ratio in [3.0, 6.0, 24.0] {
            let ring = Ring::new(32768);
            // A tone just above the destination Nyquist frequency would fold
            // directly into the audible band without sufficient anti-aliasing.
            unsafe {
                assert!(ring.write_with(32000, |i| Frame {
                    left: (i as f64 * 2.0 * std::f64::consts::PI * 0.55 / ratio).sin() as f32,
                    right: 0.0,
                }));
            }
            let mut reader = unsafe { ring.reader() };
            let mut converter = Resampler::new(ratio);
            let mut energy = 0.0;
            for i in 0..1000 {
                let sample = converter.next(&mut reader, ratio).unwrap().left;
                if i > 128 {
                    energy += sample * sample;
                }
            }
            assert!(
                energy / 871.0 < 0.00001,
                "alias energy at ratio {ratio}: {energy}"
            );
        }
    }
    #[test]
    fn recovery_and_controller_are_bounded() {
        let ring = Ring::new(256);
        unsafe {
            ring.write_with(200, |i| Frame {
                left: i as f32,
                right: 0.0,
            });
        }
        let mut reader = unsafe { ring.reader() };
        reader.keep_newest(100);
        assert_eq!(reader.available(), 100);
        assert_eq!(reader.peek(0).left, 100.0);
        let mut converter = Resampler::new(1.0);
        for _ in 0..10000 {
            assert!(converter.corrected_ratio(1.0, 1.0, 100000, 64, 0.01) <= 1.005);
        }
        while converter.next(&mut reader, 1.0).is_some() {}
        converter.reset();
        assert!(!converter.playing);
        assert_eq!(clean_gain(f32::NAN), 1.0);
        assert_eq!(clean_sample(f32::INFINITY), 0.0);
    }
}
