//! Where new ids come from.

pub mod in_memory;
pub mod time_ids;

pub trait IdGenerator {
    fn next_id(&self) -> String;
}
