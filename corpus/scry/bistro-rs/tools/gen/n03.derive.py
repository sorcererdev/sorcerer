# n03 (N, rules-in-commands), hand edit of feature base f-walkin: the table choice moves to a private helper fn in the command's module (still in the command)
sub("booking/commands/seat_walk_in.rs", """        let table = self
            .bookings
            .tables()
            .into_iter()
            .filter(|t| t.seats >= input.party)
            .filter(|t| self.bookings.booking_at(&t.id, &input.slot).is_none())
            .min_by_key(|t| t.seats);
        let Some(table) = table else {""", """        let Some(table) = smallest_free_table(&*self.bookings, input.party, &input.slot) else {""")
sub("booking/commands/seat_walk_in.rs", """pub struct SeatWalkIn {""", """/// The smallest table that seats PARTY and is free at SLOT.
fn smallest_free_table(bookings: &dyn BookingRepository, party: u32, slot: &Slot) -> Option<Table> {
    bookings
        .tables()
        .into_iter()
        .filter(|t| t.seats >= party)
        .filter(|t| bookings.booking_at(&t.id, slot).is_none())
        .min_by_key(|t| t.seats)
}

pub struct SeatWalkIn {""")
sub("booking/commands/seat_walk_in.rs", "use crate::booking::models::slot::Slot;\n", "use crate::booking::models::slot::Slot;\nuse crate::booking::models::table::Table;\n")
