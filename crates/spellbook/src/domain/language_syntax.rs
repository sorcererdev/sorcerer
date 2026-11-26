//! The language file's `[syntax]` table: what spellbook's anchors need about a language that its
//! code does not show, read from what core keeps for products:
//!
//! ```toml
//! [syntax]
//! list_separator = ","
//! ```

use sorcerer_core::domain::language::Language;

/// The separator an item takes when it goes into a list of one item or of none, where there is
/// no separator between two items to read it from. `None` when the language does not say.
///
/// Example: with `list_separator = ","`, adding `port: Port` after `self` in `(self)` gives
/// `(self, port: Port)`.
pub fn list_separator(lang: &Language) -> Option<String> {
    lang.rest
        .get("syntax")?
        .get("list_separator")?
        .as_str()
        .map(str::to_string)
}
