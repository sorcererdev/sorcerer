//! A table and how many it seats.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub id: String,
    pub seats: u32,
}
