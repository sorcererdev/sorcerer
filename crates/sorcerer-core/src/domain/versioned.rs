//! A product's versioned state as data: what a type stored with a version
//! declares, and what a read of it found.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// State stored as `{"version": VERSION, "data": …}`.
pub trait Versioned: Serialize + DeserializeOwned + Default {
    const VERSION: u32;
}

/// A read: nothing yet, the state, or why it cannot be read (it is then
/// never overwritten).
#[derive(Debug)]
pub enum Snapshot<T> {
    Absent,
    Current(T),
    Unreadable(String),
}
