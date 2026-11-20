//! A product's versioned state as data: what a type stored with a version
//! declares, and what a read of it found.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// A type that a product saves to disk with a version number beside it, as
/// `{"version": 2, "data": …}`. The number goes up when the saved shape changes, so a newer
/// sorcerer can tell an old file from a new one.
pub trait Versioned: Serialize + DeserializeOwned + Default {
    const VERSION: u32;
}

/// What reading saved state found.
///
/// `Absent`: nothing has been saved yet. `Current`: the saved state. `Unreadable`: a file is
/// there but cannot be read (a newer version, a hand edit gone wrong), with the reason; the
/// file is then left alone and never overwritten, so nothing the user had is lost.
#[derive(Debug)]
pub enum Snapshot<T> {
    Absent,
    Current(T),
    Unreadable(String),
}
