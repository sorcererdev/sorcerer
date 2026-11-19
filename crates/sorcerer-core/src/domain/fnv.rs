//! FNV-1a 64, the hash behind fingerprints, grammar cache keys and
//! recording keys. Small and stable across platforms.

const OFFSET: u64 = 14695981039346656037;
const PRIME: u64 = 1099511628211;

pub fn fnv64(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME))
}

/// Sixteen lowercase hex digits of [`fnv64`].
pub fn fnv64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv64(bytes))
}
