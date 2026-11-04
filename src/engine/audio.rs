//! Procedural WAV playback through one mixer with bounded queues and voice count.
use std::sync::{mpsc, Arc, OnceLock};
#[derive(Clone)]
pub struct Sound {
    samples: Arc<Vec<i16>>,
}
pub struct PlaySoundParams {
    pub looped: bool,
    pub volume: f32,
}
struct Voice {
    samples: Arc<Vec<i16>>,
    position: usize,
    volume: f32,
    looped: bool,
}
static MIXER: OnceLock<Option<mpsc::SyncSender<Voice>>> = OnceLock::new();
fn mixer() -> Option<&'static mpsc::SyncSender<Voice>> {
    MIXER
        .get_or_init(|| {
            if std::env::var_os("DW_HEADLESS").is_some() {
                return None;
            }
            let (tx, rx) = mpsc::sync_channel::<Voice>(32);
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            std::thread::spawn(move || {
                let Some(mut device) = native::Device::new() else {
                    eprintln!("Dice Wars: audio output is unavailable");
                    let _ = ready_tx.send(false);
                    return;
                };
                let _ = ready_tx.send(true);
                let mut voices: Vec<Voice> = Vec::new();
                let mut output = [0i16; 441];
                loop {
                    if voices.is_empty() {
                        match rx.recv() {
                            Ok(v) => voices.push(v),
                            Err(_) => break,
                        }
                    }
                    while let Ok(v) = rx.try_recv() {
                        if voices.len() == 16 {
                            voices.remove(0);
                        }
                        voices.push(v);
                    }
                    for sample in &mut output {
                        let mut sum = 0.0;
                        for v in &mut voices {
                            if v.position == v.samples.len() && v.looped {
                                v.position = 0;
                            }
                            if let Some(&s) = v.samples.get(v.position) {
                                sum += s as f32 * v.volume;
                                v.position += 1;
                            }
                        }
                        *sample = sum.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
                    }
                    voices.retain(|v| v.looped || v.position < v.samples.len());
                    if !device.write(&output) {
                        eprintln!("Dice Wars: audio output disconnected");
                        break;
                    }
                }
            });
            // Device negotiation must never indefinitely block game startup.
            if ready_rx.recv_timeout(std::time::Duration::from_millis(500)) == Ok(true) {
                Some(tx)
            } else {
                None
            }
        })
        .as_ref()
}
pub fn load_sound_from_bytes(bytes: &[u8]) -> Result<Sound, &'static str> {
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..16] != b"WAVEfmt " {
        return Err("invalid WAV");
    }
    let u16_at = |i| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
    let len = u32_at(40) as usize;
    if u32_at(16) != 16
        || u16_at(20) != 1
        || u16_at(22) != 1
        || u32_at(24) != 44100
        || u16_at(34) != 16
        || &bytes[36..40] != b"data"
        || len % 2 != 0
        || len == 0
        || len > 44100 * 2 * 10
        || bytes.len() != 44 + len
    {
        return Err("expected mono 44.1 kHz PCM16 WAV");
    }
    let _ = mixer();
    Ok(Sound {
        samples: Arc::new(
            bytes[44..]
                .chunks_exact(2)
                .map(|p| i16::from_le_bytes([p[0], p[1]]))
                .collect(),
        ),
    })
}
pub fn play_sound(sound: &Sound, params: PlaySoundParams) {
    if let Some(tx) = mixer() {
        let _ = tx.try_send(Voice {
            samples: sound.samples.clone(),
            position: 0,
            volume: params.volume.clamp(0.0, 1.0),
            looped: params.looped,
        });
    }
}
#[cfg(target_os = "linux")]
mod native {
    use std::ffi::{c_char, c_int, c_void};
    #[repr(C)]
    struct SampleSpec {
        format: c_int,
        rate: u32,
        channels: u8,
    }
    #[repr(C)]
    struct BufferAttr {
        maxlength: u32,
        tlength: u32,
        prebuf: u32,
        minreq: u32,
        fragsize: u32,
    }
    #[link(name = "libpulse-simple.so.0", kind = "dylib", modifiers = "+verbatim")]
    extern "C" {
        fn pa_simple_new(
            server: *const c_char,
            name: *const c_char,
            direction: c_int,
            device: *const c_char,
            stream: *const c_char,
            spec: *const SampleSpec,
            map: *const c_void,
            attr: *const BufferAttr,
            error: *mut c_int,
        ) -> *mut c_void;
        fn pa_simple_write(
            device: *mut c_void,
            data: *const c_void,
            bytes: usize,
            error: *mut c_int,
        ) -> c_int;
        fn pa_simple_free(device: *mut c_void);
    }
    pub struct Device(*mut c_void);
    impl Device {
        pub fn new() -> Option<Self> {
            let spec = SampleSpec {
                format: 3,
                rate: 44100,
                channels: 1,
            };
            let attr = BufferAttr {
                maxlength: 8820,
                tlength: 4410,
                // Automatic rebuffering is essential for intermittent effects.
                // With zero, the server's read cursor runs ahead during idle
                // and discards short effects written after that silence.
                prebuf: 882,
                minreq: 882,
                fragsize: u32::MAX,
            };
            // SAFETY: structs match PulseAudio's ABI and the stream owns no borrowed pointers.
            let ptr = unsafe {
                pa_simple_new(
                    std::ptr::null(),
                    c"Dice Wars".as_ptr(),
                    1,
                    std::ptr::null(),
                    c"Game sounds".as_ptr(),
                    &spec,
                    std::ptr::null(),
                    &attr,
                    std::ptr::null_mut(),
                )
            };
            if ptr.is_null() {
                None
            } else {
                Some(Self(ptr))
            }
        }
        pub fn write(&mut self, samples: &[i16]) -> bool {
            // SAFETY: samples remain valid for this synchronous write.
            unsafe {
                pa_simple_write(
                    self.0,
                    samples.as_ptr().cast(),
                    samples.len() * 2,
                    std::ptr::null_mut(),
                ) == 0
            }
        }
    }
    impl Drop for Device {
        fn drop(&mut self) {
            unsafe {
                pa_simple_free(self.0);
            }
        }
    }
}
#[cfg(windows)]
mod native {
    use std::ffi::c_void;
    #[repr(C, packed)]
    struct Format {
        tag: u16,
        channels: u16,
        rate: u32,
        byte_rate: u32,
        align: u16,
        bits: u16,
        extra: u16,
    }
    #[repr(C)]
    struct Header {
        data: *mut u8,
        length: u32,
        recorded: u32,
        user: usize,
        flags: u32,
        loops: u32,
        next: *mut Header,
        reserved: usize,
    }
    #[link(name = "winmm")]
    extern "system" {
        fn waveOutOpen(
            handle: *mut *mut c_void,
            id: u32,
            format: *const Format,
            callback: usize,
            instance: usize,
            flags: u32,
        ) -> u32;
        fn waveOutPrepareHeader(handle: *mut c_void, header: *mut Header, size: u32) -> u32;
        fn waveOutWrite(handle: *mut c_void, header: *mut Header, size: u32) -> u32;
        fn waveOutUnprepareHeader(handle: *mut c_void, header: *mut Header, size: u32) -> u32;
        fn waveOutReset(handle: *mut c_void) -> u32;
        fn waveOutClose(handle: *mut c_void) -> u32;
    }
    pub struct Device(*mut c_void);
    impl Device {
        pub fn new() -> Option<Self> {
            let f = Format {
                tag: 1,
                channels: 1,
                rate: 44100,
                byte_rate: 88200,
                align: 2,
                bits: 16,
                extra: 0,
            };
            let mut h = std::ptr::null_mut();
            if unsafe { waveOutOpen(&mut h, u32::MAX, &f, 0, 0, 0) } == 0 {
                Some(Self(h))
            } else {
                None
            }
        }
        pub fn write(&mut self, samples: &[i16]) -> bool {
            let mut data = samples.to_vec();
            let mut h = Header {
                data: data.as_mut_ptr().cast(),
                length: (data.len() * 2) as u32,
                recorded: 0,
                user: 0,
                flags: 0,
                loops: 0,
                next: std::ptr::null_mut(),
                reserved: 0,
            };
            let size = std::mem::size_of::<Header>() as u32;
            // SAFETY: header and sample buffer stay alive until playback completes or is reset.
            unsafe {
                if waveOutPrepareHeader(self.0, &mut h, size) != 0 {
                    return false;
                }
                if waveOutWrite(self.0, &mut h, size) != 0 {
                    waveOutUnprepareHeader(self.0, &mut h, size);
                    return false;
                }
                let start = std::time::Instant::now();
                while std::ptr::read_volatile(&h.flags) & 1 == 0 {
                    if start.elapsed().as_millis() > 250 {
                        waveOutReset(self.0);
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                waveOutUnprepareHeader(self.0, &mut h, size);
            }
            true
        }
    }
    impl Drop for Device {
        fn drop(&mut self) {
            unsafe {
                waveOutReset(self.0);
                waveOutClose(self.0);
            }
        }
    }
}
#[cfg(target_os = "macos")]
mod native {
    //! AudioToolbox AudioQueue. The queue's own thread pulls from a small ring;
    //! `write` blocks while the ring is full. Underruns play silence, so the
    //! queue never stops and effects after a quiet spell start immediately.
    use std::collections::VecDeque;
    use std::ffi::c_void;
    use std::sync::{Condvar, Mutex, MutexGuard};
    use std::time::{Duration, Instant};
    #[repr(C)]
    struct Format {
        rate: f64,
        id: u32,
        flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels: u32,
        bits: u32,
        reserved: u32,
    }
    #[repr(C)]
    struct Buffer {
        capacity: u32,
        data: *mut c_void,
        size: u32,
        user: *mut c_void,
        description_capacity: u32,
        descriptions: *mut c_void,
        description_count: u32,
    }
    type Queue = *mut c_void;
    type Callback = unsafe extern "C" fn(*mut c_void, Queue, *mut Buffer);
    #[link(name = "AudioToolbox", kind = "framework")]
    extern "C" {
        fn AudioQueueNewOutput(
            format: *const Format,
            callback: Callback,
            user: *mut c_void,
            run_loop: *mut c_void,
            run_loop_mode: *const c_void,
            flags: u32,
            queue: *mut Queue,
        ) -> i32;
        fn AudioQueueAllocateBuffer(queue: Queue, bytes: u32, buffer: *mut *mut Buffer) -> i32;
        fn AudioQueueEnqueueBuffer(
            queue: Queue,
            buffer: *mut Buffer,
            descriptions: u32,
            description: *const c_void,
        ) -> i32;
        fn AudioQueueStart(queue: Queue, time: *const c_void) -> i32;
        fn AudioQueueStop(queue: Queue, immediate: u8) -> i32;
        fn AudioQueueDispose(queue: Queue, immediate: u8) -> i32;
    }
    // Four 512-frame buffers (46 ms) plus a 1536-sample ring (35 ms).
    const FRAMES: usize = 512;
    const BUFFERS: usize = 4;
    const RING: usize = 1536;
    struct Shared {
        ring: Mutex<VecDeque<i16>>,
        space: Condvar,
    }
    impl Shared {
        fn lock(&self) -> MutexGuard<'_, VecDeque<i16>> {
            self.ring.lock().unwrap_or_else(|e| e.into_inner())
        }
    }
    unsafe extern "C" fn refill(user: *mut c_void, queue: Queue, buffer: *mut Buffer) {
        // SAFETY: `user` is the Shared owned by Device, freed only after AudioQueueDispose;
        // every buffer was allocated with FRAMES samples of capacity.
        unsafe {
            let shared = &*user.cast::<Shared>();
            let out = std::slice::from_raw_parts_mut((*buffer).data.cast::<i16>(), FRAMES);
            let mut ring = shared.lock();
            let n = ring.len().min(FRAMES);
            for (dst, src) in out.iter_mut().zip(ring.drain(..n)) {
                *dst = src;
            }
            drop(ring);
            out[n..].fill(0);
            shared.space.notify_one();
            (*buffer).size = (FRAMES * 2) as u32;
            AudioQueueEnqueueBuffer(queue, buffer, 0, std::ptr::null());
        }
    }
    pub struct Device {
        queue: Queue,
        shared: *mut Shared,
    }
    impl Device {
        pub fn new() -> Option<Self> {
            let format = Format {
                rate: 44100.0,
                id: u32::from_be_bytes(*b"lpcm"),
                // Signed integer | packed, native (little) endian.
                flags: 4 | 8,
                bytes_per_packet: 2,
                frames_per_packet: 1,
                bytes_per_frame: 2,
                channels: 1,
                bits: 16,
                reserved: 0,
            };
            let shared = Box::into_raw(Box::new(Shared {
                ring: Mutex::new(VecDeque::with_capacity(RING)),
                space: Condvar::new(),
            }));
            let mut queue = std::ptr::null_mut();
            // SAFETY: the format matches AudioStreamBasicDescription; a null run loop
            // runs callbacks on the queue's internal thread. On failure, nothing else
            // references `shared` once the queue is disposed.
            unsafe {
                if AudioQueueNewOutput(
                    &format,
                    refill,
                    shared.cast(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    0,
                    &mut queue,
                ) != 0
                    || queue.is_null()
                {
                    drop(Box::from_raw(shared));
                    return None;
                }
                let device = Self { queue, shared };
                for _ in 0..BUFFERS {
                    let mut buffer = std::ptr::null_mut();
                    if AudioQueueAllocateBuffer(queue, (FRAMES * 2) as u32, &mut buffer) != 0
                        || buffer.is_null()
                    {
                        return None;
                    }
                    std::ptr::write_bytes((*buffer).data.cast::<u8>(), 0, FRAMES * 2);
                    (*buffer).size = (FRAMES * 2) as u32;
                    if AudioQueueEnqueueBuffer(queue, buffer, 0, std::ptr::null()) != 0 {
                        return None;
                    }
                }
                if AudioQueueStart(queue, std::ptr::null()) != 0 {
                    return None;
                }
                Some(device)
            }
        }
        pub fn write(&mut self, samples: &[i16]) -> bool {
            // SAFETY: `shared` lives until Drop.
            let shared = unsafe { &*self.shared };
            for chunk in samples.chunks(RING / 2) {
                let mut ring = shared.lock();
                // The queue pulls every ~12 ms; two silent seconds means it is gone.
                let deadline = Instant::now() + Duration::from_secs(2);
                while ring.len() + chunk.len() > RING {
                    let now = Instant::now();
                    if now >= deadline {
                        return false;
                    }
                    ring = shared
                        .space
                        .wait_timeout(ring, deadline - now)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                ring.extend(chunk);
            }
            true
        }
    }
    impl Drop for Device {
        fn drop(&mut self) {
            // SAFETY: immediate dispose is synchronous; no callback runs after it returns.
            unsafe {
                AudioQueueStop(self.queue, 1);
                AudioQueueDispose(self.queue, 1);
                drop(Box::from_raw(self.shared));
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_wav_is_rejected() {
        assert!(load_sound_from_bytes(&[0; 44]).is_err());
        assert!(load_sound_from_bytes(b"RIFF").is_err());
    }
}
