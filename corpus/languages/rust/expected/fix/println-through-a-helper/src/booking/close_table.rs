//! Close a table for the night.

use crate::booking::models::Booking;

pub fn close_table(bookings: Vec<Booking>, table: u32) -> Vec<Booking> {
    todo!("hole audit_call: the same as println!(closing table); through the helper");
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_other_tables() {
        assert_eq!(close_table(vec![Booking { table: 1 }], 2).len(), 1);
    }
}
