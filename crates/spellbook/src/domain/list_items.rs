//! A list input's items: the pattern one item follows (`item = "{name}:{kind}:{target?}"`), and
//! a typed value (`name:text,dish:ref:dishes,notes:text?`) read into items by it.
//!
//! An item is split on `:` into its parts, named by the pattern in order; a part marked `?` in
//! the pattern may be left out (at the end only). A `?` after an item sets its `optional` flag.

use std::collections::BTreeMap;

use sorcerer_core::domain::template_blocks::Datum;

use crate::domain::inputs::is_a_name;
use crate::domain::tables::Tables;

/// The shape of one item: its parts, in order.
///
/// Example: `{name}:{kind}:{target?}` has the parts `name`, `kind` (both required) and
/// `target` (may be left out).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemPattern {
    pub parts: Vec<ItemPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemPart {
    pub name: String,
    pub required: bool,
}

/// One item read: each part's name and text, in the pattern's order (a part left out is empty),
/// and whether the item was marked `?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pub parts: Vec<(String, String)>,
    pub optional: bool,
}

/// Why an item did not read: the item as written and the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemProblem {
    pub item: String,
    pub why: String,
}

/// The flag an item's trailing `?` sets; no part may take its name.
pub const OPTIONAL: &str = "optional";

impl ItemPattern {
    /// Reads a pattern: parts separated by `:`, each `{part}` or `{part?}`, the optional ones
    /// last.
    ///
    /// Example: `ItemPattern::parse("{name}:{kind}")` has two required parts;
    /// `ItemPattern::parse("{a?}:{b}")` is refused (an optional part comes only at the end).
    pub fn parse(text: &str) -> Result<ItemPattern, String> {
        let parts = text
            .split(':')
            .map(pattern_part)
            .collect::<Result<Vec<_>, _>>()?;
        refuse_required_after_optional(&parts)?;
        refuse_repeated_parts(&parts)?;
        Ok(ItemPattern { parts })
    }

    /// The pattern as written: `{name}:{kind}:{target?}`.
    pub fn written(&self) -> String {
        self.parts
            .iter()
            .map(|part| match part.required {
                true => format!("{{{}}}", part.name),
                false => format!("{{{}?}}", part.name),
            })
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Whether the pattern has a part of this name.
    pub fn has_part(&self, name: &str) -> bool {
        self.parts.iter().any(|part| part.name == name)
    }

    /// The parts an item has in a template: the pattern's, then `optional`.
    ///
    /// Example: `{name}:{kind}` gives `["name", "kind", "optional"]`.
    pub fn item_parts(&self) -> Vec<&str> {
        self.parts
            .iter()
            .map(|part| part.name.as_str())
            .chain([OPTIONAL])
            .collect()
    }

    /// Reads a typed value into items. Items are separated by `,` and trimmed; an empty value
    /// is no items. Each part must be a name (no spaces); a part `keys_of` names must be a row
    /// key of that table.
    ///
    /// Example: with `{name}:{kind}:{target?}`, `name:text,dish:ref:dishes,notes:text?` gives
    /// three items; the first's `target` is empty and the third is optional. With
    /// `keys_of = { kind = "kinds" }` and no row `flaot` in `kinds`, `weight:flaot` is refused
    /// with "`flaot` is not a kind (one of: …)".
    pub fn items(
        &self,
        text: &str,
        keys_of: &BTreeMap<String, String>,
        tables: &Tables,
    ) -> Result<Vec<ListItem>, ItemProblem> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        text.split(',')
            .map(str::trim)
            .map(|written| {
                self.item(written, keys_of, tables)
                    .map_err(|why| ItemProblem {
                        item: written.to_string(),
                        why,
                    })
            })
            .collect()
    }

    fn item(
        &self,
        written: &str,
        keys_of: &BTreeMap<String, String>,
        tables: &Tables,
    ) -> Result<ListItem, String> {
        if written.is_empty() {
            return Err("an empty item (two commas together)".into());
        }
        let (bare, optional) = match written.strip_suffix('?') {
            Some(bare) => (bare, true),
            None => (written, false),
        };
        let given: Vec<&str> = bare.split(':').map(str::trim).collect();
        self.refuse_wrong_count(bare, given.len())?;
        let parts = self
            .parts
            .iter()
            .enumerate()
            .map(|(at, part)| {
                let text = given.get(at).copied().unwrap_or_default();
                checked_part(&part.name, text, keys_of, tables)
                    .map(|text| (part.name.clone(), text.to_string()))
            })
            .collect::<Result<_, _>>()?;
        Ok(ListItem { parts, optional })
    }

    fn refuse_wrong_count(&self, bare: &str, count: usize) -> Result<(), String> {
        let required = self.parts.iter().filter(|part| part.required).count();
        if (required..=self.parts.len()).contains(&count) {
            return Ok(());
        }
        let noun = if count == 1 { "part" } else { "parts" };
        Err(format!(
            "`{bare}` has {count} {noun}; an item is `{}`",
            self.written()
        ))
    }
}

impl ListItem {
    /// The item as a template reads it: each part as text, and `optional` as a flag.
    pub fn datum(&self) -> Datum {
        let parts = self
            .parts
            .iter()
            .map(|(name, text)| (name.clone(), Datum::Text(text.clone())))
            .chain([(OPTIONAL.to_string(), Datum::Flag(self.optional))])
            .collect();
        Datum::Record(parts)
    }

    /// The text of the part with this name.
    pub fn part(&self, name: &str) -> Option<&str> {
        self.parts
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, text)| text.as_str())
    }
}

fn pattern_part(written: &str) -> Result<ItemPart, String> {
    let inside = written
        .trim()
        .strip_prefix('{')
        .and_then(|rest| rest.strip_suffix('}'))
        .ok_or_else(|| {
            format!("`{written}` is not a part; write each part as {{name}} or {{name?}}")
        })?;
    let (name, required) = match inside.strip_suffix('?') {
        Some(name) => (name, false),
        None => (inside, true),
    };
    if !is_a_part_name(name) {
        return Err(format!("`{name}` is not a name for a part"));
    }
    if name == OPTIONAL {
        return Err(format!(
            "a part may not be named `{OPTIONAL}` (an item's trailing `?` sets it)"
        ));
    }
    Ok(ItemPart {
        name: name.to_string(),
        required,
    })
}

fn refuse_required_after_optional(parts: &[ItemPart]) -> Result<(), String> {
    let first_optional = parts.iter().position(|part| !part.required);
    match first_optional.and_then(|at| parts[at..].iter().find(|part| part.required)) {
        Some(part) => Err(format!(
            "`{{{}}}` comes after an optional part; optional parts come only at the end",
            part.name
        )),
        None => Ok(()),
    }
}

fn refuse_repeated_parts(parts: &[ItemPart]) -> Result<(), String> {
    match parts
        .iter()
        .enumerate()
        .find(|(at, part)| parts[..*at].iter().any(|before| before.name == part.name))
    {
        Some((_, part)) => Err(format!("the part `{}` is named twice", part.name)),
        None => Ok(()),
    }
}

/// A part's text: a name with no spaces (or empty, for a part left out), and a row key of its
/// table when `keys_of` names one.
fn checked_part<'p>(
    part: &str,
    text: &'p str,
    keys_of: &BTreeMap<String, String>,
    tables: &Tables,
) -> Result<&'p str, String> {
    if text.is_empty() {
        return Ok(text);
    }
    if !is_a_part_name(text) {
        return Err(format!("`{text}` is not a name"));
    }
    let Some(rows) = keys_of.get(part).and_then(|table| tables.rows(table)) else {
        return Ok(text);
    };
    if rows.contains_key(text) {
        return Ok(text);
    }
    let keys: Vec<&str> = rows.keys().map(String::as_str).collect();
    Err(format!(
        "`{text}` is not a {part} (one of: {})",
        keys.join(", ")
    ))
}

/// A name as an item's part holds it: what an ident input takes, without spaces (a space would
/// be lost or doubled when the value is split and trimmed).
fn is_a_part_name(text: &str) -> bool {
    is_a_name(text) && !text.contains(' ')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::tables::read_tables;

    fn pattern() -> ItemPattern {
        ItemPattern::parse("{name}:{kind}:{target?}").unwrap()
    }

    fn kinds() -> (BTreeMap<String, String>, Tables) {
        let tables =
            read_tables("[kinds.text]\ncode = \"String\"\n[kinds.int]\ncode = \"i64\"\n").unwrap();
        (
            BTreeMap::from([("kind".to_string(), "kinds".to_string())]),
            tables,
        )
    }

    #[test]
    fn a_pattern_reads_and_writes_back() {
        assert_eq!(pattern().written(), "{name}:{kind}:{target?}");
        assert_eq!(
            pattern().item_parts(),
            ["name", "kind", "target", "optional"]
        );
        assert!(ItemPattern::parse("{a?}:{b}").is_err());
        assert!(ItemPattern::parse("{optional}").is_err());
        assert!(ItemPattern::parse("name:{kind}").is_err());
        assert!(ItemPattern::parse("{a}:{a}").is_err());
    }

    #[test]
    fn items_read_their_parts_and_flag() {
        let (keys_of, tables) = kinds();
        let items = pattern()
            .items("name:text, price:int:x ,notes:text?", &keys_of, &tables)
            .unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].part("target"), Some(""));
        assert_eq!(items[1].part("target"), Some("x"));
        assert!(items[2].optional && !items[0].optional);
        assert_eq!(pattern().items(" ", &keys_of, &tables), Ok(vec![]));
    }

    #[test]
    fn a_bad_item_says_why() {
        let (keys_of, tables) = kinds();
        let why = |text: &str| pattern().items(text, &keys_of, &tables).unwrap_err().why;
        assert_eq!(why("w:flaot"), "`flaot` is not a kind (one of: int, text)");
        assert_eq!(why("a:text,,b:int"), "an empty item (two commas together)");
        assert_eq!(
            why("a"),
            "`a` has 1 part; an item is `{name}:{kind}:{target?}`"
        );
        assert_eq!(
            why("a:text:x:y"),
            "`a:text:x:y` has 4 parts; an item is `{name}:{kind}:{target?}`"
        );
        assert_eq!(why("Bad Name:text"), "`Bad Name` is not a name");
    }
}
