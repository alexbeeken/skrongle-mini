//! Random bytes from the operating system, in one place.
//!
//! `rand_core` 0.10 no longer carries `OsRng`; `getrandom` is the source it
//! drew from, and what argon2 draws its salts from.

/// Fills `buf` from the operating system's secure random source.
pub fn fill(buf: &mut [u8]) {
    getrandom::fill(buf).expect("the operating system's random source failed"); // panic-ok: a failing OS random source (no /dev/urandom) is not recoverable, and `OsRng` aborted on it too
}

pub fn u32() -> u32 {
    let mut b = [0u8; 4];
    fill(&mut b);
    u32::from_ne_bytes(b)
}

pub fn u64() -> u64 {
    let mut b = [0u8; 8];
    fill(&mut b);
    u64::from_ne_bytes(b)
}
