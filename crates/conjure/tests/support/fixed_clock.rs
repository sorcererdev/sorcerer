//! A clock that stands still.

use std::sync::Mutex;

use chrono::{DateTime, FixedOffset, Utc};
use conjure::ports::clock::Clock;

/// A clock that says what it is told, in a zone it is told (UTC unless set).
pub struct FixedClock {
    pub(super) at: Mutex<DateTime<Utc>>,
    zone: FixedOffset,
}

impl FixedClock {
    pub fn at(t: DateTime<Utc>) -> FixedClock {
        FixedClock {
            at: Mutex::new(t),
            zone: FixedOffset::east_opt(0).unwrap(),
        }
    }
}

impl FixedClock {
    /// The same clock in ZONE (the user's local offset).
    pub fn in_zone(mut self, zone: FixedOffset) -> FixedClock {
        self.zone = zone;
        self
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.at.lock().unwrap()
    }

    fn offset(&self) -> FixedOffset {
        self.zone
    }
}
