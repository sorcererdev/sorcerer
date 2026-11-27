//! Book a table for a party in a slot.

use std::rc::Rc;

use crate::booking::models::booking::Booking;
use crate::booking::models::slot::Slot;
use crate::booking::ports::booking_repository::BookingRepository;
use crate::booking::ports::id_generator::IdGenerator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateBookingInput {
    pub table_id: String,
    pub slot: Slot,
    pub party: u32,
    pub guest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateBookingFailure {
    SlotTaken { reason: String },
    PartyTooLarge { reason: String },
    TableNotFound { reason: String },
}

impl CreateBookingFailure {
    pub fn reason(&self) -> &str {
        match self {
            CreateBookingFailure::SlotTaken { reason }
            | CreateBookingFailure::PartyTooLarge { reason }
            | CreateBookingFailure::TableNotFound { reason } => reason,
        }
    }
}

pub struct CreateBooking {
    bookings: Rc<dyn BookingRepository>,
    ids: Rc<dyn IdGenerator>,
}

impl CreateBooking {
    pub fn new(bookings: Rc<dyn BookingRepository>, ids: Rc<dyn IdGenerator>) -> CreateBooking {
        CreateBooking { bookings, ids }
    }

    pub fn handle(&self, input: CreateBookingInput) -> Result<Booking, CreateBookingFailure> {
        let Some(table) = self.bookings.table(&input.table_id) else {
            return Err(CreateBookingFailure::TableNotFound {
                reason: format!("there is no table {}", input.table_id),
            });
        };
        if input.party > table.seats {
            return Err(CreateBookingFailure::PartyTooLarge {
                reason: format!(
                    "table {} seats {}, not {}",
                    table.id, table.seats, input.party
                ),
            });
        }
        if self.bookings.booking_at(&table.id, &input.slot).is_some() {
            return Err(CreateBookingFailure::SlotTaken {
                reason: format!("table {} is booked at {}", table.id, input.slot.start),
            });
        }
        let booking = Booking {
            id: self.ids.next_id(),
            table_id: table.id,
            slot: input.slot,
            party: input.party,
            guest: input.guest,
        };
        self.bookings.save(booking.clone());
        Ok(booking)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::booking::models::slot::{Day, Time};
    use crate::booking::models::table::Table;
    use crate::booking::ports::booking_repository::in_memory::InMemoryBookingRepository;
    use crate::booking::ports::id_generator::in_memory::SequentialIds;

    fn seven() -> Slot {
        Slot {
            day: Day::new(2026, 10, 9),
            start: Time::new(19, 0),
        }
    }

    fn command() -> (CreateBooking, Rc<InMemoryBookingRepository>) {
        let bookings = Rc::new(InMemoryBookingRepository::new(vec![Table {
            id: "t1".into(),
            seats: 4,
        }]));
        let create = CreateBooking::new(bookings.clone(), Rc::new(SequentialIds::default()));
        (create, bookings)
    }

    fn input(table_id: &str, party: u32, guest: &str) -> CreateBookingInput {
        CreateBookingInput {
            table_id: table_id.into(),
            slot: seven(),
            party,
            guest: guest.into(),
        }
    }

    #[test]
    fn books_a_free_slot() {
        let (create, bookings) = command();
        let booking = create.handle(input("t1", 2, "Ada")).unwrap();
        let expected = Booking {
            id: "b1".into(),
            table_id: "t1".into(),
            slot: seven(),
            party: 2,
            guest: "Ada".into(),
        };
        assert_eq!(booking, expected);
        assert_eq!(bookings.booking_at("t1", &seven()), Some(booking));
    }

    #[test]
    fn a_booked_slot_is_taken() {
        let (create, _) = command();
        create.handle(input("t1", 2, "Ada")).unwrap();
        let result = create.handle(input("t1", 2, "Grace"));
        assert!(matches!(
            result,
            Err(CreateBookingFailure::SlotTaken { .. })
        ));
    }

    #[test]
    fn a_party_larger_than_the_table_is_refused() {
        let (create, _) = command();
        let result = create.handle(input("t1", 6, "Ada"));
        assert!(matches!(
            result,
            Err(CreateBookingFailure::PartyTooLarge { .. })
        ));
    }

    #[test]
    fn an_unknown_table_is_not_found() {
        let (create, _) = command();
        let result = create.handle(input("t9", 2, "Ada"));
        assert!(matches!(
            result,
            Err(CreateBookingFailure::TableNotFound { .. })
        ));
    }
}
