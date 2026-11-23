//! The open holes, kept between runs.

use sorcerer_core::Result;

use crate::domain::record::OpenHole;

pub trait HolesStore: Send + Sync {
    /// The open holes saved; none when nothing was saved yet.
    fn read(&self) -> Result<Vec<OpenHole>>;
    /// Saves these as the open holes, in place of what was there.
    fn replace(&self, holes: Vec<OpenHole>) -> Result<()>;
}
