//! Where bookings and tables are kept.

pub mod file;
pub mod in_memory;

use crate::booking::models::booking::Booking;
use crate::booking::models::slot::{Day, Slot};
use crate::booking::models::table::Table;

pub trait BookingRepository {
    fn table(&self, table_id: &str) -> Option<Table>;
    fn tables(&self) -> Vec<Table>;
    fn booking_at(&self, table_id: &str, slot: &Slot) -> Option<Booking>;
    fn bookings_on(&self, day: Day) -> Vec<Booking>;
    fn save(&self, booking: Booking);
}

/// Every BookingRepository adapter keeps the same contract.
#[cfg(test)]
mod tests {
    use super::file::FileBookingRepository;
    use super::in_memory::InMemoryBookingRepository;
    use super::*;
    use crate::booking::models::slot::Time;

    fn tables() -> Vec<Table> {
        vec![
            Table {
                id: "t1".into(),
                seats: 4,
            },
            Table {
                id: "t2".into(),
                seats: 2,
            },
        ]
    }

    fn seven() -> Slot {
        Slot {
            day: Day::new(2026, 10, 9),
            start: Time::new(19, 0),
        }
    }

    fn repositories(name: &str) -> Vec<Box<dyn BookingRepository>> {
        let path = std::env::temp_dir().join(format!("bistro-{}-{name}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        vec![
            Box::new(InMemoryBookingRepository::new(tables())),
            Box::new(FileBookingRepository::new(path, tables())),
        ]
    }

    #[test]
    fn finds_a_table() {
        for repository in repositories("finds") {
            assert_eq!(
                repository.table("t2"),
                Some(Table {
                    id: "t2".into(),
                    seats: 2
                })
            );
            assert_eq!(repository.table("t9"), None);
        }
    }

    #[test]
    fn saves_and_finds_a_booking() {
        for repository in repositories("saves") {
            let booking = Booking {
                id: "b1".into(),
                table_id: "t1".into(),
                slot: seven(),
                party: 3,
                guest: "Ada".into(),
            };
            repository.save(booking.clone());
            assert_eq!(repository.booking_at("t1", &seven()), Some(booking.clone()));
            assert_eq!(repository.booking_at("t2", &seven()), None);
            assert_eq!(repository.bookings_on(seven().day), vec![booking]);
        }
    }
}
