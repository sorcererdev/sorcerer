//! A BookingRepository in memory: for tests and the first run.

use std::cell::RefCell;

use crate::booking::models::booking::Booking;
use crate::booking::models::slot::{Day, Slot};
use crate::booking::models::table::Table;
use crate::booking::ports::booking_repository::BookingRepository;

pub struct InMemoryBookingRepository {
    tables: Vec<Table>,
    bookings: RefCell<Vec<Booking>>,
}

impl InMemoryBookingRepository {
    pub fn new(tables: Vec<Table>) -> InMemoryBookingRepository {
        InMemoryBookingRepository {
            tables,
            bookings: RefCell::new(Vec::new()),
        }
    }
}

impl BookingRepository for InMemoryBookingRepository {
    fn table(&self, table_id: &str) -> Option<Table> {
        self.tables.iter().find(|t| t.id == table_id).cloned()
    }

    fn tables(&self) -> Vec<Table> {
        self.tables.clone()
    }

    fn booking_at(&self, table_id: &str, slot: &Slot) -> Option<Booking> {
        self.bookings
            .borrow()
            .iter()
            .find(|b| b.table_id == table_id && b.slot == *slot)
            .cloned()
    }

    fn bookings_on(&self, day: Day) -> Vec<Booking> {
        self.bookings
            .borrow()
            .iter()
            .filter(|b| b.slot.day == day)
            .cloned()
            .collect()
    }

    fn save(&self, booking: Booking) {
        self.bookings.borrow_mut().push(booking);
    }
}
