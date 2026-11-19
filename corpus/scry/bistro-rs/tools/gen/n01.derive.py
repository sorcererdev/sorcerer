# n01 (N, clock), hand-written on the base: a model method told the time by its caller (a pure fact about the booking; no clock read)
sub("booking/models/booking.rs", "use crate::booking::models::slot::Slot;\n", "use crate::booking::models::slot::{Day, Slot, Time};\n")
sub("booking/models/booking.rs", """    pub guest: String,
}
""", """    pub guest: String,
}

/// How long a table is held for a booking.
pub const SITTING_MINUTES: u32 = 90;

impl Booking {
    /// Whether the sitting is over at NOW on TODAY: a booking on an earlier day is over,
    /// one on a later day is not.
    pub fn is_over(&self, today: Day, now: Time) -> bool {
        if self.slot.day != today {
            return self.slot.day < today;
        }
        let start = self.slot.start.hour * 60 + self.slot.start.minute;
        now.hour * 60 + now.minute >= start + SITTING_MINUTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_seven() -> Booking {
        Booking {
            id: "b1".into(),
            table_id: "t1".into(),
            slot: Slot {
                day: Day::new(2026, 10, 9),
                start: Time::new(19, 0),
            },
            party: 2,
            guest: "Ada".into(),
        }
    }

    #[test]
    fn a_sitting_is_over_ninety_minutes_after_it_starts() {
        let day = Day::new(2026, 10, 9);
        assert!(!at_seven().is_over(day, Time::new(20, 29)));
        assert!(at_seven().is_over(day, Time::new(20, 30)));
    }

    #[test]
    fn other_days_are_over_or_not_by_their_day() {
        assert!(at_seven().is_over(Day::new(2026, 10, 10), Time::new(17, 0)));
        assert!(!at_seven().is_over(Day::new(2026, 10, 8), Time::new(23, 0)));
    }
}
""")
