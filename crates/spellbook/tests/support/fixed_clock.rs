//! A clock that always says the same time.

use chrono::{DateTime, TimeZone, Utc};
use spellbook::ports::clock::Clock;

pub struct FixedClock(pub DateTime<Utc>);

impl FixedClock {
    pub fn at_noon() -> FixedClock {
        FixedClock(Utc.with_ymd_and_hms(2026, 11, 22, 12, 0, 0).unwrap())
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        self.0
    }
}
