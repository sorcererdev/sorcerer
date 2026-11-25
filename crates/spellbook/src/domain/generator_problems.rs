//! What a generator that does not load is told: the problem, where it is, and what to write
//! instead (olifant's sentences, kept).

use std::fmt;

/// A generator that did not load: which one, which of its files, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorProblem {
    pub generator: String,
    /// The file, relative to the generator's directory: `generator.toml`,
    /// `templates/command.py.dt`.
    pub file: String,
    pub why: String,
}

impl fmt::Display for GeneratorProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "generator {} ({}): {}",
            self.generator, self.file, self.why
        )
    }
}

/// Keys olifant's generators have that arrive in a later milestone here.
const LATER_KEYS: &[&str] = &["for", "ast", "missing", "fix"];

/// What to say about a key a table does not have: the right word when it is a known slip, a
/// later milestone when it is olifant's, else the nearest known key or the list of them.
///
/// Example: `content` gives "`content` is not a key; write `text`"; `ast` gives
/// "`ast` arrives in milestone 2"; `promt` gives "`promt` is not a key; did you mean `prompt`?".
pub fn unknown_key_text(key: &str, known: &[&str]) -> String {
    if key == "content" && known.contains(&"text") {
        return "`content` is not a key; write `text`".into();
    }
    if key == "version" {
        return "`version` is not a key (spellbook's generators have no version); remove it".into();
    }
    if LATER_KEYS.contains(&key) {
        return format!("`{key}` arrives in milestone 2");
    }
    match nearest(key, known) {
        Some(near) => format!("`{key}` is not a key; did you mean `{near}`?"),
        None => format!("`{key}` is not a key (the keys are: {})", known.join(", ")),
    }
}

/// What to say about a value a key does not take.
///
/// Example: `type = "list"` gives "type = \"list\" arrives in milestone 2"; `type = "nmae"`
/// gives "type = \"nmae\" is not one of: ident, enum, text".
pub fn unknown_value_text(key: &str, value: &str, known: &[&str]) -> String {
    if key == "type" && value == "list" {
        return "type = \"list\" arrives in milestone 2".into();
    }
    format!("{key} = \"{value}\" is not one of: {}", known.join(", "))
}

/// What to say about a name in a template that looks like olifant's function style.
///
/// Example: `pascalCase(command)` gives "`pascalCase(command)` is not a name; write
/// {= command|pascal =}".
pub fn function_style_text(name: &str) -> Option<String> {
    let (function, rest) = name.split_once('(')?;
    let argument = rest.strip_suffix(')')?.trim();
    let filter = function.strip_suffix("Case")?.to_lowercase();
    Some(format!(
        "`{name}` is not a name; write {{= {argument}|{filter} =}}"
    ))
}

/// The known key closest to a slip, when it is at most two edits away.
fn nearest<'k>(key: &str, known: &[&'k str]) -> Option<&'k str> {
    known
        .iter()
        .map(|candidate| (edit_distance(key, candidate), *candidate))
        .filter(|(distance, _)| *distance <= 2)
        .min()
        .map(|(_, candidate)| candidate)
}

/// How many single-letter insertions, deletions or changes turn one word into another.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let first_row: Vec<usize> = (0..=b.len()).collect();
    a.chars()
        .enumerate()
        .fold(first_row, |above, (i, letter)| {
            let mut row = vec![i + 1];
            for (j, other) in b.iter().enumerate() {
                let change = above[j] + usize::from(letter != *other);
                row.push(change.min(above[j + 1] + 1).min(row[j] + 1));
            }
            row
        })
        .last()
        .copied()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slips_get_their_sentences() {
        assert_eq!(
            unknown_key_text("content", &["create", "text"]),
            "`content` is not a key; write `text`"
        );
        assert_eq!(
            unknown_key_text("ast", &["create"]),
            "`ast` arrives in milestone 2"
        );
        assert_eq!(
            unknown_key_text("promt", &["prompt", "type"]),
            "`promt` is not a key; did you mean `prompt`?"
        );
        assert!(unknown_key_text("zzzzzz", &["prompt"]).contains("the keys are: prompt"));
    }

    #[test]
    fn function_style_names_say_the_filter() {
        assert_eq!(
            function_style_text("pascalCase(command)").as_deref(),
            Some("`pascalCase(command)` is not a name; write {= command|pascal =}")
        );
        assert_eq!(function_style_text("command"), None);
    }
}
