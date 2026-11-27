//! Languages as data. Sorcerer's code never names a language: each one is a `language.toml`
//! and some tree-sitter query files under `crates/sorcerer-core/library/languages/<name>/`.
//!
//! A project can change a shipped language, or add one, with the same files under
//! `.sorcerer/languages/<name>/`. The two are laid one over the other, the project's on top: a
//! top-level key in the project's file replaces the shipped key whole (a table is replaced, not
//! merged key by key), and a query file with the same name replaces the shipped one while a new
//! name adds a file. Each language remembers which layer set each key and each query file, so
//! `doctor` can show it.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use globset::{Glob, GlobSet, GlobSetBuilder};
use include_dir::{Dir, include_dir};

use crate::domain::unit::is_unit_capture;
use crate::{Error, Result};

/// The shipped languages, embedded in the binary.
static SHIPPED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/library/languages");

/// The keys core reads; every other key stays in [`Language::rest`].
const CORE_KEYS: &[&str] = &[
    "files",
    "grammar",
    "comment",
    "comment_end",
    "unit_words",
    "unit_separator",
    "unit",
];

/// Where a grammar's sources come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarSource {
    /// `grammar = { git = "…", rev = "…" }`: fetched at that revision. `dir` names the folder
    /// of the repository that holds the grammar (its `src/parser.c`), for a repository that
    /// holds more than one: `grammar = { git = "…", rev = "…", dir = "tsx" }` builds
    /// `tsx/src`. Without it the grammar is the repository's own `src`.
    Git {
        url: String,
        rev: String,
        dir: Option<String>,
    },
    /// `grammar = { path = "…" }`: a directory holding `parser.c` (or a
    /// checkout with `src/parser.c`), relative to its layer's directory.
    Local(PathBuf),
}

/// Who named a language's grammar: a language sorcerer ships, or the project (its own
/// `.sorcerer/languages/<name>/language.toml`, or an override such as a test's vendored
/// sources). Only a shipped grammar may be downloaded prebuilt; the project's own is always
/// compiled on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrammarOrigin {
    Shipped,
    Project,
}

/// How a grammar's library got into the cache when this process loaded it: already there
/// (`Cached`), fetched prebuilt from scry's release (`Downloaded`), or compiled here
/// (`Built`, with the compiler's name, such as `cc`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarObtained {
    Cached,
    Downloaded,
    Built { compiler: String },
}

/// One language, its layers already laid together.
#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    /// Globs over project-relative paths: which files are this language's.
    pub files: Vec<String>,
    pub grammar: GrammarSource,
    /// How a comment starts, such as `#`, `//` or `<!--`. `None` when the language has no
    /// comments (no allow comments, no owned regions, no answer kept as a comment).
    pub comment: Option<String>,
    /// How a comment ends, for a language whose comments are closed (`-->`, `*/`); `None` for
    /// a comment that runs to the end of the line.
    pub comment_end: Option<String>,
    /// How to tell a model what a unit is called in this language, used in prompts that ask
    /// it to name one: "a top-level class or function name (Name) or a method (Class.method)".
    pub unit_words: Option<String>,
    /// Joins a unit's name to its parent's: `Order.cancel`, `Order::new`.
    pub unit_separator: String,
    /// `[unit] name`: patterns that read a unit's name from its first line, for languages whose
    /// queries do not find every unit.
    pub unit_name: Vec<String>,
    /// Query files by name (`tags.scm`) to their text.
    pub queries: BTreeMap<String, String>,
    /// Which layer set each top-level key and each query file (`queries/tags.scm`):
    /// `shipped`, `project` or `override`.
    pub origins: BTreeMap<String, String>,
    /// The keys core does not read, kept for the products that own them.
    pub rest: toml::Table,
}

/// Every language the project knows, and which one a file belongs to.
#[derive(Debug, Clone, Default)]
pub struct Languages {
    by_name: BTreeMap<String, Language>,
    globs: Vec<(String, GlobSet)>,
    pub(crate) loaded: Arc<Mutex<HashMap<String, Arc<Loaded>>>>,
}

/// A language ready to parse: its grammar loaded and its queries compiled.
pub struct Loaded {
    pub language: Language,
    pub grammar: tree_sitter::Language,
    /// How the grammar's library came to be in the cache.
    pub obtained: GrammarObtained,
    /// Each query file's name and compiled query, in name order.
    pub queries: Vec<(String, tree_sitter::Query)>,
}

impl Language {
    /// Who named this language's grammar: `Shipped` when the grammar key still comes from the
    /// language sorcerer ships, `Project` when the project's layer or an override set it.
    ///
    /// For example a project whose layer for a shipped language only adds to `files` keeps the
    /// shipped grammar, so it is `Shipped`; one whose layer sets
    /// `grammar = { path = "my-grammar" }` is `Project`.
    pub fn grammar_origin(&self) -> GrammarOrigin {
        match self.origins.get("grammar").map(String::as_str) {
            Some("shipped") => GrammarOrigin::Shipped,
            _ => GrammarOrigin::Project,
        }
    }
}

/// How a language writes a comment: what opens it and, for a closed comment such as
/// `<!-- … -->` or `/* … */`, what ends it. A line comment (`#`, `//`) has no end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommentMarks {
    pub start: String,
    pub end: Option<String>,
}

impl CommentMarks {
    /// A comment that runs to the end of its line, opened by `start`.
    pub fn line(start: &str) -> CommentMarks {
        CommentMarks {
            start: start.to_string(),
            end: None,
        }
    }

    /// The text as one comment on one line: `# note` for `#`, `<!-- note -->` for `<!--` and
    /// `-->`.
    pub fn around(&self, text: &str) -> String {
        match &self.end {
            Some(end) => format!("{} {text} {end}", self.start),
            None => format!("{} {text}", self.start),
        }
    }

    /// The text of a line that is one whole comment, without the marks and the spaces beside
    /// them; `None` when the line is not one.
    ///
    /// Example: with `<!--` and `-->`, `  <!-- spellbook:end x -->` is `spellbook:end x`; with
    /// `#`, `# note` is `note` and `x = 1` is `None`.
    pub fn inside<'a>(&self, line: &'a str) -> Option<&'a str> {
        let after_start = line.trim().strip_prefix(self.start.as_str())?;
        let inner = match &self.end {
            Some(end) => after_start.strip_suffix(end.as_str())?,
            None => after_start,
        };
        Some(inner.trim())
    }

    /// The text with a closing mark at its end taken off: `rule — why -->` is `rule — why`.
    /// A line comment's text is returned as it is.
    pub fn without_end<'a>(&self, text: &'a str) -> &'a str {
        match &self.end {
            Some(end) => text
                .trim_end()
                .strip_suffix(end.as_str())
                .map_or(text, str::trim_end),
            None => text,
        }
    }
}

impl Language {
    /// How this language writes a comment, from `comment` (and `comment_end`); `None` when it
    /// has no comments, such as a data format without any.
    pub fn comment_marks(&self) -> Option<CommentMarks> {
        Some(CommentMarks {
            start: self.comment.clone()?,
            end: self.comment_end.clone(),
        })
    }

    /// Whether this language's queries capture any unit (`@definition.function`,
    /// `@unit.section`, …), read from the query texts without loading the grammar. A language
    /// without them, such as a config format or markup, has no units: scry and conjure never
    /// load its grammar.
    pub fn has_unit_captures(&self) -> bool {
        self.queries
            .values()
            .any(|text| captures_written_in(text).any(is_unit_capture))
    }
}

/// The capture names a query text writes (`@name`, `@definition.function`), comment lines
/// (`; …`) left out.
fn captures_written_in(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with(';'))
        .flat_map(|line| line.split('@').skip(1))
        .map(|after| {
            let end = after
                .find(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '_' | '-')))
                .unwrap_or(after.len());
            &after[..end]
        })
}

impl Loaded {
    /// Parses a whole file into its syntax tree. `None` when tree-sitter gives up on it; an
    /// error only when the grammar cannot be used at all.
    pub fn parse(&self, text: &str) -> Result<Option<tree_sitter::Tree>> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&self.grammar)
            .map_err(|e| Error::GrammarLoad {
                name: self.language.name.clone(),
                why: e.to_string(),
            })?;
        Ok(parser.parse(text, None))
    }
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("language", &self.language.name)
            .finish_non_exhaustive()
    }
}

/// One layer of a language as read from disk: its `language.toml` (a local grammar path
/// already made absolute) and its query files by name.
pub(crate) struct Layer {
    pub(crate) table: toml::Table,
    pub(crate) queries: BTreeMap<String, String>,
}

impl Languages {
    /// The shipped layers, one per language, from the embedded library.
    pub(crate) fn shipped_layers() -> Result<BTreeMap<String, Vec<(&'static str, Layer)>>> {
        let mut layers: BTreeMap<String, Vec<(&str, Layer)>> = BTreeMap::new();
        for dir in SHIPPED.dirs() {
            let name = dir.path().to_string_lossy().into_owned();
            layers
                .entry(name.clone())
                .or_default()
                .push(("shipped", shipped_layer(&name, dir)?));
        }
        Ok(layers)
    }

    /// Lays each language's layers together, in the order given, and indexes the result by
    /// file pattern. Every name in `[languages] names` must have at least one layer; a language
    /// whose file is missing `files` or `grammar` is an error.
    pub(crate) fn from_layers(
        layers: BTreeMap<String, Vec<(&str, Layer)>>,
        named_in_config: &[String],
    ) -> Result<Languages> {
        if let Some(missing) = named_in_config.iter().find(|n| !layers.contains_key(*n)) {
            return Err(Error::Language {
                name: missing.clone(),
                why: "named in [languages] but no language file".into(),
            });
        }
        layers
            .into_iter()
            .try_fold(Languages::default(), |languages, (name, layers)| {
                languages.with(merge(&name, layers)?)
            })
    }

    /// These languages and one more.
    fn with(mut self, language: Language) -> Result<Languages> {
        let files = files_glob_set(&language)?;
        self.globs.push((language.name.clone(), files));
        self.by_name.insert(language.name.clone(), language);
        Ok(self)
    }

    /// The language with this name.
    pub fn get(&self, name: &str) -> Option<&Language> {
        self.by_name.get(name)
    }

    /// The language a file belongs to, from its path relative to the project root: the first
    /// language, in name order, whose `files` patterns match it.
    pub fn for_path(&self, rel: &Path) -> Option<&Language> {
        self.globs
            .iter()
            .find(|(_, g)| g.is_match(rel))
            .and_then(|(n, _)| self.by_name.get(n))
    }

    /// The language a file belongs to, as [`Languages::for_path`], but only when that language
    /// has units ([`Language::has_unit_captures`]): what scry reviews and conjure writes in. A
    /// README or a config file gives `None` without its grammar being loaded.
    pub fn with_units_for_path(&self, rel: &Path) -> Option<&Language> {
        self.for_path(rel)
            .filter(|language| language.has_unit_captures())
    }

    /// Makes a language build its grammar from other sources, such as sources vendored for
    /// tests or an offline mirror, and forgets the grammar already loaded. Does nothing when
    /// there is no language with that name.
    pub fn override_grammar(&mut self, name: &str, source: GrammarSource) {
        if let Some(l) = self.by_name.get_mut(name) {
            l.grammar = source;
            self.loaded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(name);
            l.origins.insert("grammar".into(), "override".into());
        }
    }

    /// Every language's name, in name order.
    pub fn names(&self) -> Vec<&str> {
        self.by_name.keys().map(String::as_str).collect()
    }
}

pub(crate) fn parse_toml(name: &str, text: &str) -> Result<toml::Table> {
    toml::from_str(text).map_err(|e| Error::Language {
        name: name.into(),
        why: e.message().to_string(),
    })
}

/// The `files` patterns of a language as one glob set; a bad pattern names the language.
fn files_glob_set(language: &Language) -> Result<GlobSet> {
    let bad_pattern = |e: globset::Error| Error::Language {
        name: language.name.clone(),
        why: format!("files: {e}"),
    };
    let set = language
        .files
        .iter()
        .try_fold(GlobSetBuilder::new(), |mut set, pattern| {
            set.add(Glob::new(pattern).map_err(bad_pattern)?);
            Ok::<_, Error>(set)
        })?;
    set.build().map_err(bad_pattern)
}

/// A shipped language's layer, from the directory embedded in the binary.
fn shipped_layer(name: &str, dir: &Dir<'_>) -> Result<Layer> {
    let table = match dir.get_file(dir.path().join("language.toml")) {
        Some(f) => parse_toml(name, f.contents_utf8().unwrap_or_default())?,
        None => toml::Table::new(),
    };
    Ok(Layer {
        table,
        queries: shipped_queries(dir),
    })
}

/// The `.scm` files in a shipped language's `queries/` directory, by file name.
fn shipped_queries(dir: &Dir<'_>) -> BTreeMap<String, String> {
    let Some(queries) = dir.get_dir(dir.path().join("queries")) else {
        return BTreeMap::new();
    };
    queries
        .files()
        .filter(|f| f.path().extension().is_some_and(|x| x == "scm"))
        .map(|f| {
            let file = f.path().file_name().unwrap().to_string_lossy().into_owned();
            (file, f.contents_utf8().unwrap_or_default().to_string())
        })
        .collect()
}

/// One language from its layers: the layers laid together, then core's keys read out of them.
fn merge(name: &str, layers: Vec<(&str, Layer)>) -> Result<Language> {
    let merged = LaidTogether::from(layers);
    let keys = CoreKeys {
        language: name,
        table: &merged.table,
    };
    Ok(Language {
        name: name.into(),
        files: keys.files()?,
        grammar: keys.grammar()?,
        comment: keys.string("comment")?,
        comment_end: keys.string("comment_end")?,
        unit_words: keys.string("unit_words")?,
        unit_separator: keys.string("unit_separator")?.unwrap_or_else(|| ".".into()),
        unit_name: keys.unit_name()?,
        rest: keys.not_for_core(),
        queries: merged.queries,
        origins: merged.origins,
    })
}

/// A language's layers laid one over the other, with which layer set each key and query file.
struct LaidTogether {
    table: toml::Table,
    queries: BTreeMap<String, String>,
    origins: BTreeMap<String, String>,
}

impl LaidTogether {
    /// A later layer's key or query file replaces an earlier one with the same name.
    fn from(layers: Vec<(&str, Layer)>) -> LaidTogether {
        let mut merged = LaidTogether {
            table: toml::Table::new(),
            queries: BTreeMap::new(),
            origins: BTreeMap::new(),
        };
        for (origin, layer) in layers {
            for (key, value) in layer.table {
                merged.origins.insert(key.clone(), origin.to_string());
                merged.table.insert(key, value);
            }
            for (file, text) in layer.queries {
                merged
                    .origins
                    .insert(format!("queries/{file}"), origin.to_string());
                merged.queries.insert(file, text);
            }
        }
        merged
    }
}

/// Reads core's keys out of a language's laid-together table; every error names the language.
struct CoreKeys<'a> {
    language: &'a str,
    table: &'a toml::Table,
}

impl CoreKeys<'_> {
    /// `files`: required, a list of glob patterns.
    fn files(&self) -> Result<Vec<String>> {
        let Some(files) = self.table.get("files") else {
            return Err(self.error("its language file has no files".into()));
        };
        files
            .clone()
            .try_into::<Vec<String>>()
            .map_err(|e| self.error(format!("files: {}", e.message())))
    }

    /// `grammar`: required, either `{ git, rev }` (with an optional `dir`) or `{ path }`.
    fn grammar(&self) -> Result<GrammarSource> {
        let wrong_shape =
            || self.error("grammar must be { git, rev } (with an optional dir) or { path }".into());
        match self.table.get("grammar") {
            Some(toml::Value::Table(grammar)) => grammar_of(grammar).ok_or_else(wrong_shape),
            Some(_) => Err(wrong_shape()),
            None => Err(self.error("its language file has no grammar".into())),
        }
    }

    /// An optional key that must be a string when present.
    fn string(&self, key: &str) -> Result<Option<String>> {
        match self.table.get(key) {
            None => Ok(None),
            Some(toml::Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(self.error(format!("{key} must be a string"))),
        }
    }

    /// `[unit] name`: one pattern or a list of them; none when absent.
    fn unit_name(&self) -> Result<Vec<String>> {
        let Some(toml::Value::Table(unit)) = self.table.get("unit") else {
            return Ok(Vec::new());
        };
        match unit.get("name") {
            None => Ok(Vec::new()),
            Some(toml::Value::String(pattern)) => Ok(vec![pattern.clone()]),
            Some(patterns) => patterns
                .clone()
                .try_into::<Vec<String>>()
                .map_err(|e| self.error(format!("[unit] name: {}", e.message()))),
        }
    }

    /// Every key core does not read, for the products. Keys in `[unit]` other than `name`
    /// stay, under `unit`.
    fn not_for_core(&self) -> toml::Table {
        let other_keys = self
            .table
            .iter()
            .filter(|(key, _)| !CORE_KEYS.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()));
        let other_unit_keys = match self.table.get("unit") {
            Some(toml::Value::Table(unit)) => {
                let mut others = unit.clone();
                others.remove("name");
                (!others.is_empty()).then(|| ("unit".to_string(), toml::Value::Table(others)))
            }
            _ => None,
        };
        other_keys.chain(other_unit_keys).collect()
    }

    fn error(&self, why: String) -> Error {
        Error::Language {
            name: self.language.into(),
            why,
        }
    }
}

/// The grammar's source from its table: `{ git, rev }` (with an optional `dir`) or `{ path }`,
/// nothing else. A `dir` must be a plain relative folder (`tsx`, `grammars/block`): one that
/// is absolute or climbs out with `..` is refused, like any other wrong shape.
fn grammar_of(g: &toml::Table) -> Option<GrammarSource> {
    let s = |k: &str| g.get(k).and_then(toml::Value::as_str).map(String::from);
    match (s("git"), s("rev"), s("path"), s("dir")) {
        (Some(url), Some(rev), None, dir) if dir.as_deref().is_none_or(is_plain_folder) => {
            Some(GrammarSource::Git { url, rev, dir })
        }
        (None, None, Some(path), None) => Some(GrammarSource::Local(PathBuf::from(path))),
        _ => None,
    }
}

/// A relative folder inside a repository: `tsx` or `grammars/block`, not `/abs`, `../up` or ``.
fn is_plain_folder(dir: &str) -> bool {
    !dir.is_empty()
        && Path::new(dir)
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}
