//! The system's clock and the user's local offset.

use chrono::{DateTime, FixedOffset, Local, Utc};

use crate::ports::clock::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn offset(&self) -> FixedOffset {
        *Local::now().offset()
    }
}
