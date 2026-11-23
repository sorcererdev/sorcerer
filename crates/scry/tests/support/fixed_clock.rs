//! A clock that stands still.

use std::sync::Mutex;

use chrono::{DateTime, Duration, FixedOffset, Utc};
use scry::ports::clock::Clock;

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

    /// The same clock in ZONE (the user's local offset).
    pub fn in_zone(mut self, zone: FixedOffset) -> FixedClock {
        self.zone = zone;
        self
    }

    pub fn now(&self) -> DateTime<Utc> {
        *self.at.lock().unwrap()
    }

    /// Moves the clock by D (negative goes back).
    pub fn offset(&self, d: Duration) {
        *self.at.lock().unwrap() += d;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        FixedClock::now(self)
    }

    fn offset(&self) -> FixedOffset {
        self.zone
    }
}
