//! FNV-1a 64, the hash behind fingerprints, grammar cache keys and
//! recording keys. Small and stable across platforms.

const OFFSET: u64 = 14695981039346656037;
const PRIME: u64 = 1099511628211;

/// A 64-bit hash of some bytes (the FNV-1a algorithm). The same bytes give the same number on
/// every machine and in every release, so the number can be stored and compared later.
pub fn fnv64(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME))
}

/// The same hash written as 16 lowercase hex digits, for file names and stored keys.
///
/// Example: `fnv64_hex(b"")` is `"cbf29ce484222325"`.
pub fn fnv64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv64(bytes))
}
