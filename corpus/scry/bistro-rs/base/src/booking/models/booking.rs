//! A booking: a table held for a party in a slot.

use crate::booking::models::slot::Slot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Booking {
    pub id: String,
    pub table_id: String,
    pub slot: Slot,
    pub party: u32,
    pub guest: String,
}
