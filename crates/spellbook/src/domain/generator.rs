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
    /// `[[uses]]`: other generators planned into the same plan after this one's edits.
    pub uses: Vec<Use>,
    /// Each file under `files/` (by its path there with `/`) and its bytes: what a `copy` edit
    /// writes unchanged.
    pub files: BTreeMap<String, Vec<u8>>,
    /// `[notes] after`: lines printed once the plan lands, each a template over the inputs and
    /// derived names (what to run next, a file to fill in).
    pub notes_after: Vec<String>,
}

/// `[[uses]]`: another generator planned into the same plan, its inputs templates over this
/// generator's values.
///
/// Example: `{ generator = "port", with = { port = "{= port =}" } }` plans the `port` generator
/// with this generator's `port`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    pub generator: String,
    pub with: BTreeMap<String, String>,
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
    /// One of the values the generator itself lists (`choices = ["command", "query"]`).
    Fixed { choices: Vec<String> },
    /// Any text.
    Text,
}

impl InputKind {
    /// The kind as `generator.toml` writes it.
    pub fn word(&self) -> &'static str {
        match self {
            InputKind::Ident => "ident",
            InputKind::Enum { .. } | InputKind::Fixed { .. } => "enum",
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

/// `[[edits]]`: one file to create, or lines to put in a file that is there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// `create`: a new file.
    Create {
        /// The path, a template over the inputs and derived names.
        path: String,
        body: EditBody,
        on_exists: OnExists,
    },
    /// `insert`: lines put in a file that is there (or that an earlier edit of the plan creates).
    ///
    /// Example: `insert = "{= module =}"`, `at = "end of imports"`, `text = "import os"`,
    /// `unless = { matches = "^import os$" }` adds the import once.
    Insert {
        /// The file, a template over the inputs and derived names.
        file: String,
        /// Where the lines go: phrases tried in order, the first that resolves wins.
        at: Vec<String>,
        body: EditBody,
        /// A regex (a template): when any line of the file matches it, the edit is skipped.
        unless: Option<String>,
        /// A phrase (a template): the edit happens only when it resolves in the file.
        when: Option<String>,
        on_missing: OnMissing,
    },
}

impl Edit {
    /// The path template of the file the edit writes or changes.
    pub fn path(&self) -> &str {
        match self {
            Edit::Create { path, .. } => path,
            Edit::Insert { file, .. } => file,
        }
    }

    pub fn body(&self) -> &EditBody {
        match self {
            Edit::Create { body, .. } | Edit::Insert { body, .. } => body,
        }
    }

    /// The key that names the path: `create` or `insert`.
    pub fn word(&self) -> &'static str {
        match self {
            Edit::Create { .. } => "create",
            Edit::Insert { .. } => "insert",
        }
    }
}

/// What an `insert` does when its file is not there or none of its phrases resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnMissing {
    /// Stop the plan, naming the edit and the phrase (the default).
    Error,
    /// Leave the file as it is.
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditBody {
    /// The name of a template file under `templates/`.
    Template(String),
    /// The text itself, written as it is.
    Text(String),
    /// `copy = "files/<path>"`: the file's path under `files/`, its bytes written unchanged (never
    /// rendered, so `{= =}` in it stays as it is; binary allowed).
    ///
    /// Example: `copy = "files/htmx.min.js"` is `Copy("htmx.min.js")`.
    Copy(String),
}

/// Where `copy` files sit in a generator's directory.
pub const FILES_DIR: &str = "files";

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
    "uses",
    "notes",
];
const NOTES_KEYS: &[&str] = &["after"];
const USE_KEYS: &[&str] = &["generator", "with"];
const INPUT_KEYS: &[&str] = &["type", "one_of", "choices", "prompt", "optional", "default"];
const QUERY_KEYS: &[&str] = &["glob", "value"];
const EDIT_KEYS: &[&str] = &[
    "create",
    "insert",
    "at",
    "template",
    "text",
    "copy",
    "on_exists",
    "unless",
    "when",
    "on_missing",
];
const CREATE_ONLY: &[&str] = &["on_exists", "copy"];
const INSERT_ONLY: &[&str] = &["at", "unless", "when", "on_missing"];
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
        uses: uses(&top)?,
        files: BTreeMap::new(),
        notes_after: notes_after(&top)?,
    })
}

/// `[notes] after`: the lines to print once the plan lands, in order.
fn notes_after(top: &Keys) -> Result<Vec<String>, String> {
    let Some(value) = top.table.get("notes") else {
        return Ok(Vec::new());
    };
    let table = value
        .as_table()
        .ok_or_else(|| "[notes] must be a table, `after = [\"…\"]`".to_string())?;
    let keys = Keys::new("[notes]", table);
    keys.refuse_unknown(NOTES_KEYS)?;
    keys.texts("after")
}

/// `[[uses]]`: each a generator id and its `with`.
fn uses(top: &Keys) -> Result<Vec<Use>, String> {
    let Some(value) = top.table.get("uses") else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| "uses must be written as [[uses]] tables".to_string())?;
    list.iter()
        .enumerate()
        .map(|(i, value)| {
            let place = format!("[[uses]] {}", i + 1);
            let table = value
                .as_table()
                .ok_or_else(|| format!("{place}: must be a table"))?;
            let keys = Keys::new(&place, table);
            keys.refuse_unknown(USE_KEYS)?;
            let with = match table.get("with") {
                None => BTreeMap::new(),
                Some(Value::Table(with)) => with
                    .iter()
                    .map(|(name, value)| match value {
                        Value::String(text) => Ok((name.clone(), text.clone())),
                        _ => Err(keys.at(&format!("with.{name} is a template (text)"))),
                    })
                    .collect::<Result<_, String>>()?,
                Some(_) => return Err(keys.at("with is a table of input = template")),
            };
            Ok(Use {
                generator: keys.required_text("generator")?,
                with,
            })
        })
        .collect()
}

fn input(name: &str, table: &Table) -> Result<Input, String> {
    let keys = Keys::new(&format!("[inputs.{name}]"), table);
    keys.refuse_unknown(INPUT_KEYS)?;
    let kind = match keys.required_text("type")?.as_str() {
        "ident" => InputKind::Ident,
        "text" => InputKind::Text,
        "enum" => enum_kind(&keys, table)?,
        other => {
            return Err(keys.at(&unknown_value_text(
                "type",
                other,
                &["ident", "enum", "text"],
            )));
        }
    };
    if !is_enum(&kind) {
        for key in ["one_of", "choices"] {
            if table.contains_key(key) {
                return Err(keys.at(&format!("{key} belongs to an input of type \"enum\"")));
            }
        }
    }
    let default = keys.text("default")?;
    if let (InputKind::Fixed { choices }, Some(value)) = (&kind, &default)
        && !choices.contains(value)
    {
        return Err(keys.at(&format!(
            "default \"{value}\" is not one of the choices ({})",
            choices.join(", ")
        )));
    }
    Ok(Input {
        kind,
        prompt: keys.text("prompt")?.unwrap_or_default(),
        optional: keys.flag("optional")?,
        default,
    })
}

fn is_enum(kind: &InputKind) -> bool {
    matches!(kind, InputKind::Enum { .. } | InputKind::Fixed { .. })
}

/// An enum's values come from a project query (`one_of`) or from the generator (`choices`),
/// one of the two.
fn enum_kind(keys: &Keys, table: &Table) -> Result<InputKind, String> {
    match (table.contains_key("one_of"), table.contains_key("choices")) {
        (true, true) => {
            Err(keys.at("an enum has `one_of` (a query) or `choices` (a list), not both"))
        }
        (false, true) => {
            let choices = keys.texts("choices")?;
            if choices.is_empty() {
                return Err(keys.at("choices must list at least one value"));
            }
            Ok(InputKind::Fixed { choices })
        }
        _ => Ok(InputKind::Enum {
            one_of: keys.required_text("one_of")?,
        }),
    }
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
    let body = edit_body(&keys)?;
    match (keys.text("create")?, keys.text("insert")?) {
        (Some(path), None) => {
            keys.refuse_keys_of_the_other("create", INSERT_ONLY)?;
            Ok(Edit::Create {
                path,
                body,
                on_exists: match keys.text("on_exists")?.as_deref() {
                    None | Some("error") => OnExists::Error,
                    Some("skip") => OnExists::Skip,
                    Some(other) => {
                        return Err(keys.at(&unknown_value_text(
                            "on_exists",
                            other,
                            &["error", "skip"],
                        )));
                    }
                },
            })
        }
        (None, Some(file)) => {
            keys.refuse_keys_of_the_other("insert", CREATE_ONLY)?;
            Ok(Edit::Insert {
                file,
                at: keys.phrases("at")?,
                body,
                unless: keys.inner_text("unless", "matches")?,
                when: keys.inner_text("when", "found")?,
                on_missing: match keys.text("on_missing")?.as_deref() {
                    None | Some("error") => OnMissing::Error,
                    Some("skip") => OnMissing::Skip,
                    Some(other) => {
                        return Err(keys.at(&unknown_value_text(
                            "on_missing",
                            other,
                            &["error", "skip"],
                        )));
                    }
                },
            })
        }
        (Some(_), Some(_)) => Err(keys.at("give create or insert, not both")),
        (None, None) => {
            Err(keys.at("give create (a new file) or insert (lines in a file that is there)"))
        }
    }
}

/// What an edit writes: a template, a text, or a file copied from `files/`, exactly one.
fn edit_body(keys: &Keys) -> Result<EditBody, String> {
    let copy = match keys.text("copy")? {
        Some(path) => Some(copied_file(&path).map_err(|why| keys.at(&why))?),
        None => None,
    };
    let given = [
        keys.text("template")?.map(EditBody::Template),
        keys.text("text")?.map(EditBody::Text),
        copy,
    ];
    let mut bodies = given.into_iter().flatten();
    match (bodies.next(), bodies.next()) {
        (Some(body), None) => Ok(body),
        (Some(_), Some(_)) => Err(keys.at("give one of template, text or copy")),
        (None, _) => {
            Err(keys
                .at("give template (a file under templates/), text, or copy (a file under files/)"))
        }
    }
}

/// `copy`'s value: a path under `files/`, kept as its path there.
///
/// Example: `files/fonts/inter.woff2` is `fonts/inter.woff2`; `templates/x.dt` is refused.
fn copied_file(path: &str) -> Result<EditBody, String> {
    match path.strip_prefix(&format!("{FILES_DIR}/")) {
        Some(inside) if !inside.is_empty() && !inside.split('/').any(|part| part == "..") => {
            Ok(EditBody::Copy(inside.to_string()))
        }
        _ => Err(format!(
            "copy = \"{path}\": write a path under {FILES_DIR}/ (copy = \"{FILES_DIR}/<file>\")"
        )),
    }
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

    /// A key of the other kind of edit: `on_exists` in an `insert`, `at` in a `create`.
    fn refuse_keys_of_the_other(&self, word: &str, others: &[&str]) -> Result<(), String> {
        match others.iter().find(|key| self.table.contains_key(**key)) {
            Some(key) => Err(self.at(&format!("`{key}` does not go with {word}"))),
            None => Ok(()),
        }
    }

    /// `at`: one phrase or a list of them, at least one.
    fn phrases(&self, key: &str) -> Result<Vec<String>, String> {
        let phrases = match self.table.get(key) {
            None => Vec::new(),
            Some(Value::String(one)) => vec![one.clone()],
            Some(_) => self.texts(key)?,
        };
        if phrases.is_empty() {
            return Err(self.at(&format!(
                "{key} is missing: where the lines go (\"end of imports\", or a list tried in order)"
            )));
        }
        Ok(phrases)
    }

    /// A one-key table's text: `unless = { matches = "…" }`.
    fn inner_text(&self, key: &str, inner: &str) -> Result<Option<String>, String> {
        let Some(value) = self.table.get(key) else {
            return Ok(None);
        };
        let shape = || self.at(&format!("{key} is a table, `{{ {inner} = \"…\" }}`"));
        let table = value.as_table().ok_or_else(shape)?;
        match (table.len(), table.get(inner)) {
            (1, Some(Value::String(text))) => Ok(Some(text.clone())),
            _ => Err(shape()),
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
