//! Cancel a booking by its id.

use std::rc::Rc;

use crate::booking::models::booking::Booking;
use crate::booking::ports::booking_repository::BookingRepository;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelBookingInput {
    pub booking_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelBookingFailure {
    BookingNotFound { reason: String },
}

impl CancelBookingFailure {
    pub fn reason(&self) -> &str {
        match self {
            CancelBookingFailure::BookingNotFound { reason } => reason,
        }
    }
}

pub struct CancelBooking {
    bookings: Rc<dyn BookingRepository>,
}

impl CancelBooking {
    pub fn new(bookings: Rc<dyn BookingRepository>) -> CancelBooking {
        CancelBooking { bookings }
    }

    pub fn handle(&self, input: CancelBookingInput) -> Result<Booking, CancelBookingFailure> {
        let Some(booking) = self.bookings.booking(&input.booking_id) else {
            return Err(CancelBookingFailure::BookingNotFound {
                reason: format!("there is no booking {}", input.booking_id),
            });
        };
        self.bookings.delete(&booking.id);
        Ok(booking)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::booking::models::slot::{Day, Slot, Time};
    use crate::booking::models::table::Table;
    use crate::booking::ports::booking_repository::in_memory::InMemoryBookingRepository;

    fn seven() -> Slot {
        Slot {
            day: Day::new(2026, 10, 9),
            start: Time::new(19, 0),
        }
    }

    fn command() -> (CancelBooking, Rc<InMemoryBookingRepository>) {
        let bookings = Rc::new(InMemoryBookingRepository::new(vec![Table {
            id: "t1".into(),
            seats: 4,
        }]));
        bookings.save(Booking {
            id: "b1".into(),
            table_id: "t1".into(),
            slot: seven(),
            party: 2,
            guest: "Ada".into(),
        });
        (CancelBooking::new(bookings.clone()), bookings)
    }

    fn input(booking_id: &str) -> CancelBookingInput {
        CancelBookingInput {
            booking_id: booking_id.into(),
        }
    }

    #[test]
    fn cancels_a_booking_and_frees_the_slot() {
        let (cancel, bookings) = command();
        let booking = cancel.handle(input("b1")).unwrap();
        assert_eq!(booking.id, "b1");
        assert_eq!(bookings.booking_at("t1", &seven()), None);
    }

    #[test]
    fn an_unknown_booking_is_not_found() {
        let (cancel, bookings) = command();
        let result = cancel.handle(input("b9"));
        assert!(matches!(
            result,
            Err(CancelBookingFailure::BookingNotFound { .. })
        ));
        assert!(bookings.booking("b1").is_some());
    }
}
