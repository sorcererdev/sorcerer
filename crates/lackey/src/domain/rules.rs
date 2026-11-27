//! The project's rules as context for a task, from scry's steering file
//! `.sorcerer/scry.toml`. lackey reads three keys only (`context`, `include`, `[[rule]] text`)
//! and ignores the rest: scry owns that file and checks it.

use std::path::Path;

/// The rules block is cut at 3000 characters.
pub const RULES_CAP: usize = 3000;

/// The steering's prose as one text: its `context`, the text of each `include` (the files are
/// read by `include`; one it cannot read is skipped), then each rule's `text`, separated by
/// blank lines. Cut at 3000 characters with a last line saying so.
///
/// Example: `context = "A booking app."` and one rule `text = "Money is cents."` give
/// `A booking app.\n\n- Money is cents.`.
pub fn rules_text(
    toml_text: &str,
    include: impl Fn(&Path) -> Option<String>,
) -> Result<String, String> {
    let table: toml::Table = toml::from_str(toml_text).map_err(|e| e.message().to_string())?;
    let parts: Vec<String> = context_of(&table)
        .into_iter()
        .chain(included_texts(&table, &include))
        .chain(rule_texts(&table))
        .collect();
    Ok(cut_at_cap(&parts.join("\n\n")))
}

fn context_of(table: &toml::Table) -> Option<String> {
    let context = table.get("context")?.as_str()?.trim();
    (!context.is_empty()).then(|| context.to_string())
}

fn included_texts(table: &toml::Table, include: &impl Fn(&Path) -> Option<String>) -> Vec<String> {
    let paths = table.get("include").and_then(toml::Value::as_array);
    paths
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .filter_map(|path| include(Path::new(path)))
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect()
}

fn rule_texts(table: &toml::Table) -> Vec<String> {
    let rules = table.get("rule").and_then(toml::Value::as_array);
    rules
        .into_iter()
        .flatten()
        .filter_map(|rule| rule.get("text")?.as_str())
        .map(|text| format!("- {}", text.trim()))
        .collect()
}

fn cut_at_cap(text: &str) -> String {
    if text.chars().count() <= RULES_CAP {
        return text.to_string();
    }
    let kept: String = text.chars().take(RULES_CAP).collect();
    format!(
        "{}\n… (rules cut at {RULES_CAP} characters)",
        kept.trim_end()
    )
}
