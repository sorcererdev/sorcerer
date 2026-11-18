//! Languages as data. A language is layers of TOML merged in order
//! (shipped, then the project's `.sorcerer/languages/<name>/`) plus its
//! query files; the engine reads only the merged result and never names a
//! language.
//!
//! Merge rule: a top-level key in a later layer replaces the earlier key
//! whole (tables are not deep-merged); query files merge by file name (the
//! same name replaces, a new name adds). `origins` says which layer set
//! each key and each query file, for `doctor`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use include_dir::{Dir, include_dir};

use crate::{Error, Result};

/// The shipped languages, embedded in the binary.
static SHIPPED: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../library/languages");

/// The keys core reads; every other key stays in [`Language::rest`].
const CORE_KEYS: &[&str] = &[
    "files",
    "grammar",
    "comment",
    "unit_words",
    "unit_separator",
    "unit",
];

/// Where a grammar's sources come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarSource {
    /// `grammar = { git = "…", rev = "…" }`: fetched at that revision.
    Git { url: String, rev: String },
    /// `grammar = { path = "…" }`: a directory holding `parser.c` (or a
    /// checkout with `src/parser.c`), relative to its layer's directory.
    Local(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Language {
    pub name: String,
    /// Globs over project-relative paths: which files are this language's.
    pub files: Vec<String>,
    pub grammar: GrammarSource,
    pub comment: Option<String>,
    pub unit_words: Option<String>,
    /// Joins a unit's name to its parent's: `Order.cancel`, `Order::new`.
    pub unit_separator: String,
    /// `[unit] name`: patterns reading a unit's name from its first line.
    pub unit_name: Vec<String>,
    /// Query files by name (`tags.scm`) to their text.
    pub queries: BTreeMap<String, String>,
    /// Each top-level key, and each `queries/<file>`, to the layer that set it.
    pub origins: BTreeMap<String, String>,
    /// Tables core does not read, for the products that own them.
    pub rest: toml::Table,
}

#[derive(Debug, Clone, Default)]
pub struct Languages {
    by_name: BTreeMap<String, Language>,
    globs: Vec<(String, GlobSet)>,
}

/// One layer as read: its table (grammar paths made absolute) and queries.
struct Layer {
    table: toml::Table,
    queries: BTreeMap<String, String>,
}

impl Languages {
    /// The shipped languages, layered with the project's under
    /// `<root>/.sorcerer/languages/`. Every name in EXTRA (`[languages]
    /// names`) must have a layer somewhere.
    pub fn assemble(root: &Path, extra: &[String]) -> Result<Languages> {
        let mut layers: BTreeMap<String, Vec<(&str, Layer)>> = BTreeMap::new();
        for dir in SHIPPED.dirs() {
            let name = dir.path().to_string_lossy().into_owned();
            layers
                .entry(name.clone())
                .or_default()
                .push(("shipped", shipped_layer(&name, dir)?));
        }
        let project = root.join(".sorcerer/languages");
        if let Ok(entries) = std::fs::read_dir(&project) {
            for e in entries.flatten().filter(|e| e.path().is_dir()) {
                let name = e.file_name().to_string_lossy().into_owned();
                layers
                    .entry(name.clone())
                    .or_default()
                    .push(("project", project_layer(&name, &e.path())?));
            }
        }
        if let Some(missing) = extra.iter().find(|n| !layers.contains_key(*n)) {
            return Err(Error::Language {
                name: missing.clone(),
                why: "named in [languages] but no language file".into(),
            });
        }
        let mut out = Languages::default();
        for (name, ls) in layers {
            let lang = merge(&name, ls)?;
            let mut set = GlobSetBuilder::new();
            for g in &lang.files {
                set.add(Glob::new(g).map_err(|e| Error::Language {
                    name: name.clone(),
                    why: format!("files: {e}"),
                })?);
            }
            let set = set.build().map_err(|e| Error::Language {
                name: name.clone(),
                why: format!("files: {e}"),
            })?;
            out.globs.push((name.clone(), set));
            out.by_name.insert(name, lang);
        }
        Ok(out)
    }

    pub fn get(&self, name: &str) -> Option<&Language> {
        self.by_name.get(name)
    }

    /// The first language, in name order, whose `files` match REL.
    pub fn for_path(&self, rel: &Path) -> Option<&Language> {
        self.globs
            .iter()
            .find(|(_, g)| g.is_match(rel))
            .and_then(|(n, _)| self.by_name.get(n))
    }

    /// Points NAME's grammar at SOURCE (vendored sources in tests, an
    /// offline mirror). Does nothing when there is no such language.
    pub fn override_grammar(&mut self, name: &str, source: GrammarSource) {
        if let Some(l) = self.by_name.get_mut(name) {
            l.grammar = source;
            l.origins.insert("grammar".into(), "override".into());
        }
    }

    pub fn names(&self) -> Vec<&str> {
        self.by_name.keys().map(String::as_str).collect()
    }
}

fn parse_toml(name: &str, text: &str) -> Result<toml::Table> {
    toml::from_str(text).map_err(|e| Error::Language {
        name: name.into(),
        why: e.message().to_string(),
    })
}

fn shipped_layer(name: &str, dir: &Dir<'_>) -> Result<Layer> {
    let table = match dir.get_file(dir.path().join("language.toml")) {
        Some(f) => parse_toml(name, f.contents_utf8().unwrap_or_default())?,
        None => toml::Table::new(),
    };
    let mut queries = BTreeMap::new();
    if let Some(q) = dir.get_dir(dir.path().join("queries")) {
        for f in q
            .files()
            .filter(|f| f.path().extension().is_some_and(|x| x == "scm"))
        {
            let file = f.path().file_name().unwrap().to_string_lossy().into_owned();
            queries.insert(file, f.contents_utf8().unwrap_or_default().to_string());
        }
    }
    Ok(Layer { table, queries })
}

fn project_layer(name: &str, dir: &Path) -> Result<Layer> {
    let file = dir.join("language.toml");
    let mut table = match std::fs::read_to_string(&file) {
        Ok(text) => parse_toml(name, &text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(e) => return Err(Error::io(file, e)),
    };
    // A local grammar's path is relative to this layer's directory.
    if let Some(toml::Value::Table(g)) = table.get_mut("grammar")
        && let Some(toml::Value::String(p)) = g.get("path")
    {
        let abs = dir.join(p).to_string_lossy().into_owned();
        g.insert("path".into(), toml::Value::String(abs));
    }
    let mut queries = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(dir.join("queries")) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "scm") {
                let text = std::fs::read_to_string(&p).map_err(|err| Error::io(&p, err))?;
                queries.insert(e.file_name().to_string_lossy().into_owned(), text);
            }
        }
    }
    Ok(Layer { table, queries })
}

fn merge(name: &str, layers: Vec<(&str, Layer)>) -> Result<Language> {
    let mut table = toml::Table::new();
    let mut queries = BTreeMap::new();
    let mut origins = BTreeMap::new();
    for (origin, layer) in layers {
        for (k, v) in layer.table {
            origins.insert(k.clone(), origin.to_string());
            table.insert(k, v);
        }
        for (k, v) in layer.queries {
            origins.insert(format!("queries/{k}"), origin.to_string());
            queries.insert(k, v);
        }
    }
    let bad = |why: String| Error::Language {
        name: name.into(),
        why,
    };
    let files = match table.get("files") {
        Some(v) => v
            .clone()
            .try_into::<Vec<String>>()
            .map_err(|e| bad(format!("files: {}", e.message())))?,
        None => return Err(bad("its language file has no files".into())),
    };
    let grammar = match table.get("grammar") {
        Some(toml::Value::Table(g)) => {
            grammar_of(g).ok_or_else(|| bad("grammar must be { git, rev } or { path }".into()))?
        }
        Some(_) => return Err(bad("grammar must be { git, rev } or { path }".into())),
        None => return Err(bad("its language file has no grammar".into())),
    };
    let string = |k: &str| -> Result<Option<String>> {
        match table.get(k) {
            None => Ok(None),
            Some(toml::Value::String(s)) => Ok(Some(s.clone())),
            Some(_) => Err(bad(format!("{k} must be a string"))),
        }
    };
    let comment = string("comment")?;
    let unit_words = string("unit_words")?;
    let unit_separator = string("unit_separator")?.unwrap_or_else(|| ".".into());
    let mut rest: toml::Table = table
        .iter()
        .filter(|(k, _)| !CORE_KEYS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let mut unit_name = Vec::new();
    if let Some(toml::Value::Table(unit)) = table.get("unit") {
        let mut others = unit.clone();
        match others.remove("name") {
            None => {}
            Some(toml::Value::String(s)) => unit_name.push(s),
            Some(v) => {
                unit_name = v
                    .try_into::<Vec<String>>()
                    .map_err(|e| bad(format!("[unit] name: {}", e.message())))?
            }
        }
        if !others.is_empty() {
            rest.insert("unit".into(), toml::Value::Table(others));
        }
    }
    Ok(Language {
        name: name.into(),
        files,
        grammar,
        comment,
        unit_words,
        unit_separator,
        unit_name,
        queries,
        origins,
        rest,
    })
}

fn grammar_of(g: &toml::Table) -> Option<GrammarSource> {
    let s = |k: &str| g.get(k).and_then(toml::Value::as_str).map(String::from);
    match (s("git"), s("rev"), s("path")) {
        (Some(url), Some(rev), None) => Some(GrammarSource::Git { url, rev }),
        (None, None, Some(path)) => Some(GrammarSource::Local(PathBuf::from(path))),
        _ => None,
    }
}
