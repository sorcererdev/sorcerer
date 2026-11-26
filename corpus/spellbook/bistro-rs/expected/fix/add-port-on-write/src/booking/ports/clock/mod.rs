//! Clock.

pub mod in_memory;

pub trait Clock {
    fn todo_method(&self) -> ();
}
