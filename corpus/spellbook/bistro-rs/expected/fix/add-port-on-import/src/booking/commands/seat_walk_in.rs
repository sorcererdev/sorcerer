//! Seat a party that walks in: a booking with a new id, for the table and slot they take.

use std::rc::Rc;

use crate::booking::models::booking::Booking;
use crate::booking::models::slot::Slot;
use crate::booking::ports::id_generator::IdGenerator;
use crate::booking::ports::clock::Clock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatWalkInInput {
    pub table_id: String,
    pub slot: Slot,
    pub party: u32,
    pub guest: String,
}

pub struct SeatWalkIn {
    ids: Rc<dyn IdGenerator>,
    clock: Rc<dyn Clock>,
}

impl SeatWalkIn {
    pub fn new(ids: Rc<dyn IdGenerator>, clock: Rc<dyn Clock>) -> SeatWalkIn {
        SeatWalkIn { ids, clock }
    }

    pub fn handle(&self, input: SeatWalkInInput) -> Booking {
        Booking {
            id: self.ids.next_id(),
            table_id: input.table_id,
            slot: input.slot,
            party: input.party,
            guest: input.guest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::booking::models::slot::{Day, Time};
    use crate::booking::ports::id_generator::in_memory::SequentialIds;

    fn command() -> SeatWalkIn {
        SeatWalkIn::new(Rc::new(SequentialIds::default()), todo!("hole fake: an in-memory adapter of the port, for the test"))
    }

    #[test]
    fn a_walk_in_gets_the_next_id() {
        let input = SeatWalkInInput {
            table_id: "t1".into(),
            slot: Slot {
                day: Day::new(2026, 10, 9),
                start: Time::new(19, 0),
            },
            party: 2,
            guest: "Ada".into(),
        };
        assert_eq!(command().handle(input).id, "b1");
    }
}
