//! What a day looks like: the tables and that day's bookings (a query).

use std::rc::Rc;

use crate::booking::models::booking::Booking;
use crate::booking::models::slot::Day;
use crate::booking::models::table::Table;
use crate::booking::ports::booking_repository::BookingRepository;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaySchedule {
    pub day: Day,
    pub tables: Vec<Table>,
    pub bookings: Vec<Booking>,
}

pub struct GetDaySchedule {
    bookings: Rc<dyn BookingRepository>,
}

impl GetDaySchedule {
    pub fn new(bookings: Rc<dyn BookingRepository>) -> GetDaySchedule {
        GetDaySchedule { bookings }
    }

    pub fn handle(&self, day: Day) -> DaySchedule {
        DaySchedule {
            day,
            tables: self.bookings.tables(),
            bookings: self.bookings.bookings_on(day),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::booking::models::slot::{Slot, Time};
    use crate::booking::ports::booking_repository::in_memory::InMemoryBookingRepository;

    #[test]
    fn lists_the_tables_and_the_days_bookings() {
        let day = Day::new(2026, 10, 9);
        let tables = vec![
            Table {
                id: "t1".into(),
                seats: 4,
            },
            Table {
                id: "t2".into(),
                seats: 2,
            },
        ];
        let bookings = Rc::new(InMemoryBookingRepository::new(tables.clone()));
        let ada = Booking {
            id: "b1".into(),
            table_id: "t1".into(),
            slot: Slot {
                day,
                start: Time::new(19, 0),
            },
            party: 2,
            guest: "Ada".into(),
        };
        let other_day = Booking {
            id: "b2".into(),
            slot: Slot {
                day: Day::new(2026, 10, 10),
                start: Time::new(19, 0),
            },
            guest: "Grace".into(),
            ..ada.clone()
        };
        bookings.save(ada.clone());
        bookings.save(other_day);
        let schedule = GetDaySchedule::new(bookings).handle(day);
        assert_eq!(
            schedule,
            DaySchedule {
                day,
                tables,
                bookings: vec![ada]
            }
        );
    }
}
