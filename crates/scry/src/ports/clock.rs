//! The time, and the user's offset (a day's cap is a local day).

use chrono::{DateTime, FixedOffset, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn offset(&self) -> FixedOffset;
}
