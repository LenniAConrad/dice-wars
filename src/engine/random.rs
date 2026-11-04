//! PCG-XSH-RR for seeded maps; OS entropy for network credentials.
use std::sync::atomic::{AtomicU64, Ordering};
static STATE: AtomicU64 = AtomicU64::new(0);
const MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const INCREMENT: u64 = 1_442_695_040_888_963_407;

pub fn rand() -> u32 {
    let state = STATE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |s| {
            Some(s.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT))
        })
        .unwrap();
    let bits = ((state ^ (state >> 18)) >> 27) as u32;
    bits.rotate_right((state >> 59) as u32)
}
pub fn srand(seed: u64) {
    STATE.store(0, Ordering::Relaxed);
    rand();
    STATE.fetch_add(seed, Ordering::Relaxed);
    rand();
}
/// Reseed from OS entropy. Called once a seeded map is dealt, so dice rolls
/// and bot choices cannot be predicted from the public map seed.
pub fn srand_from_entropy() {
    // Headless test runs can pin the dice to compare frames across runs.
    if std::env::var_os("DW_HEADLESS").is_some() {
        if let Some(seed) = std::env::var("DW_DICE_SEED")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            srand(seed);
            return;
        }
    }
    let mut bytes = [0u8; 8];
    let seed = match fill_secure(&mut bytes) {
        Ok(()) => u64::from_le_bytes(bytes),
        Err(_) => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64),
    };
    srand(seed);
}
pub trait RandomRange {
    fn from_unit(low: Self, high: Self, unit: f64) -> Self;
}
macro_rules! ranges {
    ($($t:ty),*) => { $(impl RandomRange for $t {
        fn from_unit(low: Self, high: Self, unit: f64) -> Self { (low as f64 + (high as f64 - low as f64) * unit) as Self }
    })* };
}
ranges!(f32, u32, usize, i32);
pub fn gen_range<T: RandomRange>(low: T, high: T) -> T {
    T::from_unit(low, high, rand() as f64 / 4_294_967_296.0)
}

pub fn fill_secure(bytes: &mut [u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Read;
        std::fs::File::open("/dev/urandom")?.read_exact(bytes)
    }
    #[cfg(windows)]
    {
        #[link(name = "bcrypt")]
        extern "system" {
            fn BCryptGenRandom(
                algorithm: *mut std::ffi::c_void,
                buffer: *mut u8,
                len: u32,
                flags: u32,
            ) -> i32;
        }
        let len = u32::try_from(bytes.len())
            .map_err(|_| std::io::Error::other("entropy request too large"))?;
        // SAFETY: buffer is writable for len bytes; SYSTEM_PREFERRED_RNG uses a null algorithm handle.
        let status = unsafe { BCryptGenRandom(std::ptr::null_mut(), bytes.as_mut_ptr(), len, 2) };
        if status >= 0 {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "Windows entropy failure: {status:#x}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seeded_stream_is_repeatable() {
        srand(777);
        let stream = (0..10).map(|_| rand()).collect::<Vec<_>>();
        srand(777);
        assert_eq!(stream, (0..10).map(|_| rand()).collect::<Vec<_>>());
        for _ in 0..1000 {
            assert!((2..9).contains(&gen_range(2usize, 9)));
        }
    }
    #[test]
    fn secure_entropy_fills_the_request() {
        let mut data = [0; 32];
        fill_secure(&mut data).unwrap();
        assert_ne!(data, [0; 32]);
    }
}
