//! A clock that stands still.

use apprentice::ports::clock::Clock;
use chrono::{DateTime, FixedOffset, Utc};

pub struct FixedClock(pub DateTime<Utc>);

impl FixedClock {
    /// 26 November 2026, 20:00 UTC.
    pub fn thanksgiving() -> FixedClock {
        FixedClock("2026-11-26T20:00:00Z".parse().unwrap())
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }

    fn offset(&self) -> FixedOffset {
        FixedOffset::east_opt(0).unwrap()
    }
}
