//! A generator: what `generator.toml` says, read strictly. The keys are olifant's (the M1 ones);
//! a key that is not one gets a sentence saying what to write instead
//! ([`crate::domain::generator_problems`]). The checks that look across the file (inputs against
//! queries, holes against the language, templates against the names) are in
//! [`crate::domain::generator_checks`].
//!
//! ```toml
//! name     = "Command"
//! summary  = "A command of a service, with its test"
//! language = "toy"
//!
//! [inputs.command]
//! type   = "ident"
//! prompt = "The command, a verb phrase"
//!
//! [[edits]]
//! create   = "src/{= command|snake =}.toy"
//! template = "command.toy.dt"
//! ```

use std::collections::BTreeMap;

use regex::Regex;
use toml::{Table, Value};

use crate::domain::generator_problems::{unknown_key_text, unknown_value_text};

/// One generator, its file read and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generator {
    /// The directory's name: what `spellbook new <id>` takes.
    pub id: String,
    pub name: String,
    pub summary: String,
    pub description: String,
    /// The language the generator is for (a project language), shown by `list` and `show`.
    pub language: String,
    pub examples: Vec<String>,
    /// The inputs in the file's order, which is the order they are asked in.
    pub inputs: Vec<(String, Input)>,
    pub queries: BTreeMap<String, Query>,
    /// The derived names in the file's order: each may use the inputs and the ones before it.
    pub derive: Vec<(String, String)>,
    pub edits: Vec<Edit>,
    /// The holes `[holes.X]` fills from the inputs.
    pub holes: BTreeMap<String, HoleSource>,
    /// Files a filler may read (olifant's key), kept for later milestones.
    pub context: Vec<String>,
    /// Each template file's name (under `templates/`) and its text.
    pub templates: BTreeMap<String, String>,
}

/// One input a person gives (or the project gives, when an enum has one choice).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Input {
    pub kind: InputKind,
    pub prompt: String,
    pub optional: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    /// A name in any case style (`cancel_table`, `CancelTable`), kept as its words.
    Ident,
    /// One of the values a query reads from the project.
    Enum { one_of: String },
    /// Any text.
    Text,
}

impl InputKind {
    /// The kind as `generator.toml` writes it.
    pub fn word(&self) -> &'static str {
        match self {
            InputKind::Ident => "ident",
            InputKind::Enum { .. } => "enum",
            InputKind::Text => "text",
        }
    }
}

/// `[query.X]`: paths in the project matching a glob, each read as a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// A template over the inputs given so far; `*` is one path segment.
    pub glob: String,
    pub value: QueryValue,
}

/// What a matched path gives: `src/bistro/main.py` gives `bistro` (its parent folder's name),
/// `main` (its stem) or `src/bistro/main.py` (the path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryValue {
    ParentFolder,
    Stem,
    Path,
}

/// `[[edits]]`: one file to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The path, a template over the inputs and derived names.
    pub create: String,
    pub body: EditBody,
    pub on_exists: OnExists,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditBody {
    /// The name of a template file under `templates/`.
    Template(String),
    /// The text itself, written as it is.
    Text(String),
}

/// What to do when the file to create is already there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnExists {
    /// Stop the plan before anything is written (the default).
    Error,
    /// Leave the file as it is and say so.
    Skip,
}

/// `[holes.X]`: how a hole is filled without anyone writing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoleSource {
    /// From a template over the inputs and derived names.
    Derived { value: String },
}

const TOP_KEYS: &[&str] = &[
    "name",
    "summary",
    "description",
    "language",
    "examples",
    "inputs",
    "query",
    "derive",
    "edits",
    "holes",
    "context",
];
const INPUT_KEYS: &[&str] = &["type", "one_of", "prompt", "optional", "default"];
const QUERY_KEYS: &[&str] = &["glob", "value"];
const EDIT_KEYS: &[&str] = &["create", "template", "text", "on_exists"];
const HOLE_KEYS: &[&str] = &["source", "value"];

/// Reads a generator from its `generator.toml` text and its templates, key by key. The first
/// problem is the error, as one sentence naming where it is (`[inputs.app] type: …`).
///
/// Example: a file with `content = ""` in an edit is refused with
/// `[[edits]] 1: `content` is not a key; write `text``.
pub fn read_generator(
    id: &str,
    text: &str,
    templates: BTreeMap<String, String>,
) -> Result<Generator, String> {
    let table: Table = toml::from_str(text).map_err(|e| e.message().to_string())?;
    let top = Keys::new("", &table);
    top.refuse_unknown(TOP_KEYS)?;
    Ok(Generator {
        id: id.to_string(),
        name: top.text("name")?.unwrap_or_else(|| id.to_string()),
        summary: top.text("summary")?.unwrap_or_default(),
        description: top.text("description")?.unwrap_or_default(),
        language: top.required_text("language")?,
        examples: top.texts("examples")?,
        inputs: in_file_order(&top.tables("inputs")?, &section_order(text, "inputs"))
            .into_iter()
            .map(|(name, table)| Ok((name.clone(), input(&name, table)?)))
            .collect::<Result<_, String>>()?,
        queries: top
            .tables("query")?
            .into_iter()
            .map(|(name, table)| Ok((name.clone(), query(name, table)?)))
            .collect::<Result<_, String>>()?,
        derive: derive(text, &top)?,
        edits: edits(&top)?,
        holes: top
            .tables("holes")?
            .into_iter()
            .map(|(name, table)| Ok((name.clone(), hole(name, table)?)))
            .collect::<Result<_, String>>()?,
        context: top.texts("context")?,
        templates,
    })
}

fn input(name: &str, table: &Table) -> Result<Input, String> {
    let keys = Keys::new(&format!("[inputs.{name}]"), table);
    keys.refuse_unknown(INPUT_KEYS)?;
    let kind = match keys.required_text("type")?.as_str() {
        "ident" => InputKind::Ident,
        "text" => InputKind::Text,
        "enum" => InputKind::Enum {
            one_of: keys.required_text("one_of")?,
        },
        other => {
            return Err(keys.at(&unknown_value_text(
                "type",
                other,
                &["ident", "enum", "text"],
            )));
        }
    };
    if !matches!(kind, InputKind::Enum { .. }) && table.contains_key("one_of") {
        return Err(keys.at("one_of belongs to an input of type \"enum\""));
    }
    Ok(Input {
        kind,
        prompt: keys.text("prompt")?.unwrap_or_default(),
        optional: keys.flag("optional")?,
        default: keys.text("default")?,
    })
}

fn query(name: &str, table: &Table) -> Result<Query, String> {
    let keys = Keys::new(&format!("[query.{name}]"), table);
    keys.refuse_unknown(QUERY_KEYS)?;
    let value = match keys.required_text("value")?.as_str() {
        "parent folder" => QueryValue::ParentFolder,
        "stem" => QueryValue::Stem,
        "path" => QueryValue::Path,
        other => {
            return Err(keys.at(&unknown_value_text(
                "value",
                other,
                &["parent folder", "stem", "path"],
            )));
        }
    };
    Ok(Query {
        glob: keys.required_text("glob")?,
        value,
    })
}

fn derive(text: &str, top: &Keys) -> Result<Vec<(String, String)>, String> {
    let Some(value) = top.table.get("derive") else {
        return Ok(Vec::new());
    };
    let table = value
        .as_table()
        .ok_or_else(|| "[derive] must be a table of names".to_string())?;
    let keys = Keys::new("[derive]", table);
    let order = derive_order(text);
    in_file_order(&table.iter().collect(), &order)
        .into_iter()
        .map(|(name, _)| Ok((name.clone(), keys.required_text(&name)?)))
        .collect()
}

fn edits(top: &Keys) -> Result<Vec<Edit>, String> {
    let Some(value) = top.table.get("edits") else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| "edits must be written as [[edits]] tables".to_string())?;
    list.iter()
        .enumerate()
        .map(|(i, value)| {
            let table = value
                .as_table()
                .ok_or_else(|| format!("[[edits]] {}: must be a table", i + 1))?;
            edit(i + 1, table)
        })
        .collect()
}

fn edit(number: usize, table: &Table) -> Result<Edit, String> {
    let keys = Keys::new(&format!("[[edits]] {number}"), table);
    keys.refuse_unknown(EDIT_KEYS)?;
    let body = match (keys.text("template")?, keys.text("text")?) {
        (Some(template), None) => EditBody::Template(template),
        (None, Some(text)) => EditBody::Text(text),
        (Some(_), Some(_)) => return Err(keys.at("give template or text, not both")),
        (None, None) => return Err(keys.at("give template (a file under templates/) or text")),
    };
    let on_exists = match keys.text("on_exists")?.as_deref() {
        None | Some("error") => OnExists::Error,
        Some("skip") => OnExists::Skip,
        Some(other) => {
            return Err(keys.at(&unknown_value_text("on_exists", other, &["error", "skip"])));
        }
    };
    Ok(Edit {
        create: keys.required_text("create")?,
        body,
        on_exists,
    })
}

fn hole(name: &str, table: &Table) -> Result<HoleSource, String> {
    let keys = Keys::new(&format!("[holes.{name}]"), table);
    keys.refuse_unknown(HOLE_KEYS)?;
    match keys.required_text("source")?.as_str() {
        "derived" => Ok(HoleSource::Derived {
            value: keys.required_text("value")?,
        }),
        other => Err(keys.at(&format!(
            "source = \"{other}\" is not one spellbook knows; M1 has source = \"derived\", and \
             a hole someone fills is written in the template as {{= ? {name}: kind | what belongs there =}}"
        ))),
    }
}

/// The names of `[inputs.X]` (or another section) in the order the file writes their headers.
/// The workspace's `toml` sorts a table's keys, so the order is read from the text.
fn section_order(text: &str, section: &str) -> Vec<String> {
    let header = Regex::new(&format!(r"(?m)^\s*\[{section}\.([A-Za-z0-9_-]+)\]")).expect("regex");
    header
        .captures_iter(text)
        .map(|found| found[1].to_string())
        .collect()
}

/// The names `[derive]` sets, in the order the file writes them.
fn derive_order(text: &str) -> Vec<String> {
    let line = Regex::new(r"^\s*([A-Za-z0-9_]+)\s*=").expect("regex");
    text.lines()
        .skip_while(|row| row.trim() != "[derive]")
        .skip(1)
        .take_while(|row| !row.trim_start().starts_with('['))
        .filter_map(|row| line.captures(row).map(|found| found[1].to_string()))
        .collect()
}

/// The entries in the order given, then any the order missed, in name order.
fn in_file_order<V: Copy>(entries: &BTreeMap<&String, V>, order: &[String]) -> Vec<(String, V)> {
    let ordered = order
        .iter()
        .filter_map(|name| entries.get_key_value(name).map(|(k, v)| ((*k).clone(), *v)));
    let missed = entries
        .iter()
        .filter(|(name, _)| !order.contains(name))
        .map(|(k, v)| ((*k).clone(), *v));
    ordered.chain(missed).collect()
}

/// One table's keys, read with errors that name where they are.
struct Keys<'t> {
    place: String,
    table: &'t Table,
}

impl<'t> Keys<'t> {
    fn new(place: &str, table: &'t Table) -> Keys<'t> {
        Keys {
            place: place.to_string(),
            table,
        }
    }

    /// A sentence prefixed with where it is: `[inputs.app] type: …`.
    fn at(&self, why: &str) -> String {
        if self.place.is_empty() {
            why.to_string()
        } else {
            format!("{}: {why}", self.place)
        }
    }

    fn refuse_unknown(&self, known: &[&str]) -> Result<(), String> {
        match self.table.keys().find(|key| !known.contains(&key.as_str())) {
            Some(key) => Err(self.at(&unknown_key_text(key, known))),
            None => Ok(()),
        }
    }

    fn text(&self, key: &str) -> Result<Option<String>, String> {
        match self.table.get(key) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            Some(_) => Err(self.at(&format!("{key} must be text"))),
        }
    }

    fn required_text(&self, key: &str) -> Result<String, String> {
        self.text(key)?
            .ok_or_else(|| self.at(&format!("{key} is missing")))
    }

    fn flag(&self, key: &str) -> Result<bool, String> {
        match self.table.get(key) {
            None => Ok(false),
            Some(Value::Boolean(on)) => Ok(*on),
            Some(_) => Err(self.at(&format!("{key} must be true or false"))),
        }
    }

    fn texts(&self, key: &str) -> Result<Vec<String>, String> {
        let Some(value) = self.table.get(key) else {
            return Ok(Vec::new());
        };
        value
            .as_array()
            .and_then(|list| {
                list.iter()
                    .map(|item| item.as_str().map(str::to_string))
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| self.at(&format!("{key} must be a list of text")))
    }

    fn tables(&self, key: &str) -> Result<BTreeMap<&'t String, &'t Table>, String> {
        let Some(value) = self.table.get(key) else {
            return Ok(BTreeMap::new());
        };
        let table = value
            .as_table()
            .ok_or_else(|| self.at(&format!("{key} must be written as [{key}.NAME] tables")))?;
        table
            .iter()
            .map(|(name, entry)| match entry {
                Value::Table(inner) => Ok((name, inner)),
                _ => Err(self.at(&format!("[{key}.{name}] must be a table"))),
            })
            .collect()
    }
}
