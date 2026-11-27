//! A booking and the table it holds.

pub struct Booking {
    pub table: u32,
}

impl Booking {
    pub fn is_for(&self, table: u32) -> bool {
        self.table == table
    }
}
