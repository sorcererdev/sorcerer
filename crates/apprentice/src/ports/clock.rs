//! The clock: the time now, and the user's time zone, since the caps and `apprentice cost`
//! count the user's local days.

use chrono::{DateTime, FixedOffset, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    /// The user's time zone, as an offset from UTC.
    fn offset(&self) -> FixedOffset;
}
