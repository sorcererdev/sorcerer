//! Inputs: what a person typed (`--set command=cancel_table`), checked and completed in the
//! generator's order, then the derived names computed from them. An input that cannot be had
//! stops the generation with a sentence saying what to give ([`InputStop`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use globset::GlobBuilder;
use sorcerer_core::domain::name_forms::words_of;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::templates::fill;

use crate::domain::generator::{Generator, QueryValue};

/// What the person gave, by input name, as typed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Given(pub BTreeMap<String, String>);

impl Given {
    /// The given inputs from `name=value` pairs; a pair without `=` is returned as the error.
    ///
    /// Example: `["service=booking", "command=cancel_table"]` gives both inputs.
    pub fn from_pairs(pairs: &[String]) -> Result<Given, String> {
        pairs
            .iter()
            .map(|pair| {
                pair.split_once('=')
                    .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
                    .ok_or_else(|| pair.clone())
            })
            .collect::<Result<_, _>>()
            .map(Given)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// These inputs with the built-in `timestamp` set to `now` (UTC, `YYYYMMDDHHMMSS`), unless
    /// it was given already: `--set timestamp=…` wins, so a test or a re-run can fix it.
    ///
    /// Example: at 2026-11-22 12:00:00 UTC, with nothing given, `timestamp` is `20261122120000`.
    pub fn with_timestamp(&self, now: DateTime<Utc>) -> Given {
        self.with_timestamp_text(&now.format("%Y%m%d%H%M%S").to_string())
    }

    /// These inputs with `timestamp` set to this text, unless it was given already.
    pub fn with_timestamp_text(&self, timestamp: &str) -> Given {
        let mut given = self.0.clone();
        given
            .entry(TIMESTAMP.to_string())
            .or_insert_with(|| timestamp.to_string());
        Given(given)
    }
}

/// The built-in input every generator may use without declaring it: the time the plan was made,
/// UTC, as `YYYYMMDDHHMMSS` (`20261122120000`), the way migration tools name their files.
/// A generator that declares its own `timestamp` input has that instead.
pub const TIMESTAMP: &str = "timestamp";

/// The inputs resolved, then the derived names, in order: what a template may use. An ident is
/// kept as it was typed; a filter gives its other forms, so `CancelTable` and `cancel_table`
/// fill `{= command|snake =}` the same.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values {
    entries: Vec<(String, String)>,
    inputs: usize,
}

impl Values {
    /// Inputs resolved so far, in the generator's order.
    pub fn of_inputs(entries: Vec<(String, String)>) -> Values {
        let inputs = entries.len();
        Values { entries, inputs }
    }

    /// Every name and value, as [`fill`] takes them.
    pub fn pairs(&self) -> Vec<(&str, &str)> {
        self.entries
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect()
    }

    /// The inputs alone (not the derived names), by name: what a generation records.
    pub fn inputs(&self) -> BTreeMap<String, String> {
        self.entries[..self.inputs].iter().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(known, _)| known == name)
            .map(|(_, value)| value.as_str())
    }

    /// These values and the built-in `timestamp` from what was given, unless the generator
    /// declares an input of that name (then it is among the inputs already). It is not an input:
    /// a generation's record leaves it out, as it does the derived names.
    pub fn with_built_ins(self, generator: &Generator, given: &Given) -> Values {
        let declared = generator.inputs.iter().any(|(name, _)| name == TIMESTAMP);
        match given.get(TIMESTAMP) {
            Some(timestamp) if !declared => {
                let mut entries = self.entries;
                entries.push((TIMESTAMP.to_string(), timestamp.to_string()));
                Values {
                    entries,
                    inputs: self.inputs,
                }
            }
            _ => self,
        }
    }

    /// These values and the generator's `[derive]` names, each filled with everything before it.
    ///
    /// Example: with `command = cancel_table`, `Command = "{= command|pascal =}"` adds
    /// `Command = CancelTable`.
    pub fn with_derived(self, generator: &Generator) -> Result<Values, String> {
        generator
            .derive
            .iter()
            .try_fold(self, |values, (name, template)| {
                let value = fill(template, &values.pairs())
                    .map_err(|why| format!("[derive] {name}: {why}"))?;
                let mut entries = values.entries;
                entries.push((name.clone(), value));
                Ok(Values {
                    entries,
                    inputs: values.inputs,
                })
            })
    }
}

/// Why the inputs could not all be had.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputStop {
    /// Nobody gave it, and the project does not decide it.
    Missing {
        input: String,
        prompt: String,
        choices: Vec<String>,
    },
    /// The value given is not one the project offers.
    NotAChoice {
        input: String,
        given: String,
        choices: Vec<String>,
    },
    /// The query behind an enum found nothing in the project.
    NoChoices { input: String, query: String },
    /// An ident that is not a name.
    NotAName { input: String, given: String },
    /// A template over the inputs did not fill (a query's glob, a derived name).
    Template(String),
}

impl InputStop {
    /// The input it is about, when there is one.
    pub fn input(&self) -> Option<&str> {
        match self {
            InputStop::Missing { input, .. }
            | InputStop::NotAChoice { input, .. }
            | InputStop::NoChoices { input, .. }
            | InputStop::NotAName { input, .. } => Some(input),
            InputStop::Template(_) => None,
        }
    }

    /// The values the person may choose from, when the input is an enum.
    pub fn choices(&self) -> &[String] {
        match self {
            InputStop::Missing { choices, .. } | InputStop::NotAChoice { choices, .. } => choices,
            _ => &[],
        }
    }
}

/// The stop as one sentence.
///
/// Example: `service: the service it belongs to (booking, kitchen)`.
pub fn input_stop_text(stop: &InputStop) -> String {
    match stop {
        InputStop::Missing {
            input,
            prompt,
            choices,
        } => missing_text(input, prompt, choices),
        InputStop::NotAChoice {
            input,
            given,
            choices,
        } => format!("{input}: \"{given}\" is not one of: {}", choices.join(", ")),
        InputStop::NoChoices { input, query } => {
            format!("{input}: nothing to choose from (the query {query} found nothing)")
        }
        InputStop::NotAName { input, given } => {
            format!("{input}: \"{given}\" is not a name (letters, digits, _, - and spaces)")
        }
        InputStop::Template(why) => why.clone(),
    }
}

fn missing_text(input: &str, prompt: &str, choices: &[String]) -> String {
    let prompt = lowercase_first(if prompt.is_empty() { "a value" } else { prompt });
    if choices.is_empty() {
        format!("{input}: {prompt}")
    } else {
        format!("{input}: {prompt} ({})", choices.join(", "))
    }
}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether a value given for an ident is a name: some words, and only letters, digits, `_`,
/// `-` and spaces.
pub fn is_a_name(value: &str) -> bool {
    !words_of(value).iter().all(String::is_empty)
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' '))
}

/// The values a query finds among the project's files: every file, and every folder holding
/// one, whose path matches the glob and none of the `exclude` globs, read as the query says,
/// sorted and each once. In the globs `*` stays inside one folder and `**` crosses folders.
///
/// Example: with `src/bistro/main.py` among the files, `src/*/main.py` read as the parent folder
/// gives `["bistro"]`; `src/bistro/*/models` matches the folder `src/bistro/booking/models`.
/// With `src/ports/*.rs` and the exclude `src/ports/mod.rs`, a file `mod.rs` is not a match.
pub fn query_choices(
    files: &[PathBuf],
    glob: &str,
    exclude: &[String],
    value: QueryValue,
) -> Result<Vec<String>, String> {
    let matcher = matcher_of(glob)?;
    let excluded = exclude
        .iter()
        .map(|pattern| matcher_of(pattern))
        .collect::<Result<Vec<_>, _>>()?;
    let mut found: Vec<String> = files
        .iter()
        .flat_map(|file| file.ancestors().filter(|path| !path.as_os_str().is_empty()))
        .filter(|path| matcher.is_match(path))
        .filter(|path| !excluded.iter().any(|skip| skip.is_match(path)))
        .map(|path| value_of(path, value))
        .collect();
    found.sort();
    found.dedup();
    Ok(found)
}

fn matcher_of(glob: &str) -> Result<globset::GlobMatcher, String> {
    GlobBuilder::new(glob)
        .literal_separator(true)
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|e| format!("the glob `{glob}`: {e}"))
}

fn value_of(path: &Path, value: QueryValue) -> String {
    let name_of = |part: Option<&std::ffi::OsStr>| {
        part.map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match value {
        QueryValue::ParentFolder => name_of(path.parent().and_then(Path::file_name)),
        QueryValue::Stem => name_of(path.file_stem()),
        QueryValue::Path => forward_slashes(path),
    }
}
