//! The real clock: the time now, and the user's time zone as the machine
//! has it.

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
