//! The language file's `[holes]` table: the code spellbook writes where a generator leaves a
//! hole, one template per kind, read strictly from what core keeps for products:
//!
//! ```toml
//! [holes]
//! ident = "todo_{= name =}"
//! block = "fail(\"hole {= name =}: {= instruction =}\")"
//! ```

use std::collections::BTreeMap;

use sorcerer_core::domain::language::Language;
use sorcerer_core::domain::templates::fill;
use sorcerer_core::{Error, Result};

/// `[holes]`: each hole kind and the template of its placeholder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HolesTable {
    pub kinds: BTreeMap<String, String>,
}

/// The language's `[holes]` table; empty when the language has none. A value that is not text
/// is an error naming the language and the kind.
pub fn holes_table(lang: &Language) -> Result<HolesTable> {
    let Some(value) = lang.rest.get("holes") else {
        return Ok(HolesTable::default());
    };
    let table = value
        .as_table()
        .ok_or_else(|| language_error(lang, "[holes] must be a table".into()))?;
    let kinds = table
        .iter()
        .map(|(kind, template)| match template.as_str() {
            Some(text) => Ok((kind.clone(), text.to_string())),
            None => Err(language_error(
                lang,
                format!("[holes] {kind} must be text (a placeholder template)"),
            )),
        })
        .collect::<Result<_>>()?;
    Ok(HolesTable { kinds })
}

/// The code that stands in a hole until someone fills it: the kind's template with the hole's
/// name and instruction filled in. A kind the language lacks is an error that lists its kinds.
///
/// Example: with `block = "fail(\"hole {= name =}: {= instruction =}\")"`, the hole `handle`
/// with the instruction `its first case` gives `fail("hole handle: its first case")`.
pub fn placeholder(lang: &Language, kind: &str, name: &str, instruction: &str) -> Result<String> {
    let table = holes_table(lang)?;
    let template = table
        .kinds
        .get(kind)
        .ok_or_else(|| language_error(lang, missing_kind_text(&lang.name, kind, &table)))?;
    fill(template, &[("name", name), ("instruction", instruction)])
        .map_err(|why| language_error(lang, format!("[holes] {kind}: {why}")))
}

/// Why a kind is missing, with the kinds the language has.
///
/// Example: `the language toy has no hole kind expr (it has: block, ident)`.
pub fn missing_kind_text(language: &str, kind: &str, table: &HolesTable) -> String {
    let known: Vec<&str> = table.kinds.keys().map(String::as_str).collect();
    let has = if known.is_empty() {
        "it has no [holes] table".to_string()
    } else {
        format!("it has: {}", known.join(", "))
    };
    format!("the language {language} has no hole kind {kind} ({has})")
}

fn language_error(lang: &Language, why: String) -> Error {
    Error::Language {
        name: lang.name.clone(),
        why,
    }
}
