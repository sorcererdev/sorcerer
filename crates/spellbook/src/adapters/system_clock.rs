//! The real clock.

use chrono::{DateTime, Utc};

use crate::ports::clock::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
