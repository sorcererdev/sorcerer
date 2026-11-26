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

    /// Whether a language covers the file (by its path relative to the root).
    fn covers(&self, file: &Path) -> bool {
        matches!(self.for_path(file), Ok(Some(_)))
    }

    /// How a line comment starts in the file's language (`#`, `//`), from the language's data;
    /// `None` when no language covers the file or its language has none.
    fn line_comment(&self, file: &Path) -> Option<String> {
        self.for_path(file).ok().flatten()?.language.comment.clone()
    }
}

/// A borrowed set of grammars is one too, so an adapter can own its grammars or borrow them.
impl<T: Grammars + ?Sized> Grammars for &T {
    fn for_path(&self, file: &Path) -> Result<Option<Arc<Loaded>>> {
        (**self).for_path(file)
    }

    fn covers(&self, file: &Path) -> bool {
        (**self).covers(file)
    }

    fn line_comment(&self, file: &Path) -> Option<String> {
        (**self).line_comment(file)
    }
}
