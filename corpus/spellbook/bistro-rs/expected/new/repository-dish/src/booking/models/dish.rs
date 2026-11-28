//! A dish, as it is stored.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dish {
    pub name: String,
    pub price_cents: i64,
    /// Defaults to false.
    pub available: bool,
    pub notes: Option<String>,
}

/// The table's columns, in the struct's order.
pub const COLUMNS: &str = "name, price_cents, available, notes";

impl Dish {
    /// Whether the record may be saved.
    pub fn is_valid(&self) -> bool {
        todo!("hole valid: when the record may be saved")
    }
}
