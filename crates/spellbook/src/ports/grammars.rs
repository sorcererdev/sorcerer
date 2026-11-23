//! The grammars a plan parses its files with. The use cases may not load a grammar themselves
//! (loading is core's adapter work), so they ask through this port.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::Result;
use sorcerer_core::domain::language::Loaded;

pub trait Grammars: Send + Sync {
    /// The language a file belongs to (by its path relative to the root), ready to parse; `None`
    /// when no language covers the file. An error when the grammar cannot be had.
    fn for_path(&self, file: &Path) -> Result<Option<Arc<Loaded>>>;
}
