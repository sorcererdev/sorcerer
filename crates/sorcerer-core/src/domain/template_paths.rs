//! What a template body's values and conditions read: typed data ([`Datum`]) by name
//! ([`Bindings`]), and the paths into it (`f.name`, `loop.last`, `kinds[f.kind].rust`).

use std::collections::BTreeMap;
use std::fmt;

/// One value a template body may read: text, a flag (yes or no), a list of values, or a record
/// of named values.
///
/// Example: an item of a list input `name:text?` is the record
/// `{ name: Text("name"), kind: Text("text"), optional: Flag(true) }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Datum {
    Text(String),
    Flag(bool),
    List(Vec<Datum>),
    Record(BTreeMap<String, Datum>),
}

impl Datum {
    /// Whether a condition that names this value alone holds: a flag is itself, text holds when
    /// it is not empty, a list when it has an item, a record always.
    ///
    /// Example: `Text("")` is false, `List(vec![])` is false, `Flag(true)` is true.
    pub fn holds(&self) -> bool {
        match self {
            Datum::Text(text) => !text.is_empty(),
            Datum::Flag(flag) => *flag,
            Datum::List(items) => !items.is_empty(),
            Datum::Record(_) => true,
        }
    }

    /// What sort of value it is, as an error names it: `text`, `a flag`, `a list`, `a record`.
    pub fn sort(&self) -> &'static str {
        match self {
            Datum::Text(_) => "text",
            Datum::Flag(_) => "a flag",
            Datum::List(_) => "a list",
            Datum::Record(_) => "a record",
        }
    }
}

/// The values a template body may read, by name.
///
/// Example: `Bindings::from_pairs(&[("model", "dish")]).with("fields", Datum::List(vec![]))`
/// holds the text `model` and the empty list `fields`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bindings(pub BTreeMap<String, Datum>);

impl Bindings {
    /// Bindings in which every pair is text.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Bindings {
        Bindings(
            pairs
                .iter()
                .map(|(name, value)| ((*name).to_string(), Datum::Text((*value).to_string())))
                .collect(),
        )
    }

    /// These bindings and one more, replacing any of the same name.
    pub fn with(mut self, name: &str, datum: Datum) -> Bindings {
        self.0.insert(name.to_string(), datum);
        self
    }
}

/// A path into the values: a name, then parts (`.name`) and lookups by key (`[path]`), and the
/// template line it sits on (from 1).
///
/// Example: `kinds[f.kind].rust` is the root `kinds`, a lookup by the path `f.kind`, then the
/// part `rust`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path<'t> {
    pub root: &'t str,
    pub steps: Vec<Step<'t>>,
    pub line: usize,
}

/// One step of a [`Path`] after its root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<'t> {
    /// `.name`: a part of a record.
    Field(&'t str),
    /// `[path]`: the record's entry whose name is the text the inner path gives.
    Key(Path<'t>),
}

impl fmt::Display for Path<'_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{}", self.root)?;
        self.steps.iter().try_for_each(|step| match step {
            Step::Field(name) => write!(out, ".{name}"),
            Step::Key(key) => write!(out, "[{key}]"),
        })
    }
}

impl Path<'_> {
    /// The path as written up to (not including) the step at `upto`.
    ///
    /// Example: for `kinds[f.kind].rust`, `written_upto(1)` is `kinds[f.kind]`.
    pub fn written_upto(&self, upto: usize) -> String {
        let shorter = Path {
            root: self.root,
            steps: self.steps[..upto].to_vec(),
            line: self.line,
        };
        shorter.to_string()
    }
}

/// Reads a path as written, on the given template line; `None` when the text is not one.
/// A name is letters, digits, `_` and `-`.
///
/// Example: `parse_path("kinds[f.kind].rust", 3)` is the path rooted at `kinds`, line 3;
/// `parse_path("a b", 1)` is `None`.
pub fn parse_path(text: &str, line: usize) -> Option<Path<'_>> {
    match path_prefix(text, line)? {
        (path, "") => Some(path),
        _ => None,
    }
}

/// The path at the start of the text and the text after it.
fn path_prefix(text: &str, line: usize) -> Option<(Path<'_>, &str)> {
    let (root, mut rest) = name_prefix(text)?;
    let mut steps = Vec::new();
    loop {
        if let Some(after_dot) = rest.strip_prefix('.') {
            let (field, after) = name_prefix(after_dot)?;
            steps.push(Step::Field(field));
            rest = after;
        } else if let Some(after_bracket) = rest.strip_prefix('[') {
            let (key, after) = path_prefix(after_bracket, line)?;
            rest = after.strip_prefix(']')?;
            steps.push(Step::Key(key));
        } else {
            return Some((Path { root, steps, line }, rest));
        }
    }
}

/// The name at the start of the text and the text after it.
fn name_prefix(text: &str) -> Option<(&str, &str)> {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(text.len());
    (end > 0).then(|| text.split_at(end))
}

/// Where a path's root is found: the loops in force (innermost first), then the bindings.
#[derive(Debug, Clone)]
pub(crate) struct Lookup<'b> {
    bindings: &'b Bindings,
    /// Loop names and their values, outermost first: each loop's item and its `loop` record.
    locals: Vec<(String, Datum)>,
    items: Vec<String>,
}

impl<'b> Lookup<'b> {
    pub(crate) fn new(bindings: &'b Bindings) -> Lookup<'b> {
        Lookup {
            bindings,
            locals: Vec::new(),
            items: Vec::new(),
        }
    }

    /// This lookup inside one turn of a loop: the item under its name, and `loop` with `first`,
    /// `last` and `index` (from 1).
    ///
    /// Example: the second of three items gives `loop = { first: no, last: no, index: "2" }`.
    pub(crate) fn inside_loop(&self, item: &str, datum: Datum, at: usize, count: usize) -> Self {
        let turn = Datum::Record(BTreeMap::from([
            ("first".to_string(), Datum::Flag(at == 0)),
            ("last".to_string(), Datum::Flag(at + 1 == count)),
            ("index".to_string(), Datum::Text((at + 1).to_string())),
        ]));
        let mut inner = self.clone();
        inner.locals.push((item.to_string(), datum));
        inner.locals.push(("loop".to_string(), turn));
        inner.items.push(item.to_string());
        inner
    }

    /// The value a path names, or a sentence saying where it went wrong.
    pub(crate) fn resolve(&self, path: &Path<'_>, template: &str) -> Result<&Datum, String> {
        let root = self.root(path, template)?;
        path.steps
            .iter()
            .enumerate()
            .try_fold(root, |datum, (at, step)| {
                self.step_into(datum, step, path, at, template)
            })
    }

    /// The value a name has, by itself: a loop's item or `loop`, else a binding.
    pub(crate) fn named(&self, name: &str) -> Option<&Datum> {
        self.locals
            .iter()
            .rev()
            .find(|(known, _)| known == name)
            .map(|(_, datum)| datum)
            .or_else(|| self.bindings.0.get(name))
    }

    fn root(&self, path: &Path<'_>, template: &str) -> Result<&Datum, String> {
        self.named(path.root).ok_or_else(|| {
            format!(
                "`{}` (line {}) is not a name a template may use (in `{template}`)",
                path.root, path.line
            )
        })
    }

    fn step_into<'d>(
        &'d self,
        datum: &'d Datum,
        step: &Step<'_>,
        path: &Path<'_>,
        at: usize,
        template: &str,
    ) -> Result<&'d Datum, String> {
        let name = match step {
            Step::Field(field) => (*field).to_string(),
            Step::Key(key) => self.key_text(key, path, template)?,
        };
        let Datum::Record(entries) = datum else {
            return Err(format!(
                "`{path}` (line {}): `{}` is {}, it has no parts",
                path.line,
                path.written_upto(at),
                datum.sort()
            ));
        };
        entries
            .get(&name)
            .ok_or_else(|| self.no_such_part_text(path, at, &name, entries))
    }

    fn key_text(&self, key: &Path<'_>, path: &Path<'_>, template: &str) -> Result<String, String> {
        match self.resolve(key, template)? {
            Datum::Text(text) => Ok(text.clone()),
            other => Err(format!(
                "`{path}` (line {}): the key `{key}` is {}, not text",
                path.line,
                other.sort()
            )),
        }
    }

    fn no_such_part_text(
        &self,
        path: &Path<'_>,
        at: usize,
        name: &str,
        entries: &BTreeMap<String, Datum>,
    ) -> String {
        let known: Vec<&str> = entries.keys().map(String::as_str).collect();
        let is_an_item = at == 0 && self.items.iter().any(|item| item == path.root);
        let owner = if is_an_item {
            "an item".to_string()
        } else {
            format!("`{}`", path.written_upto(at))
        };
        format!(
            "`{path}` (line {}): {owner} has no `{name}` (it has {})",
            path.line,
            known.join(", ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_prints_as_written() {
        let path = parse_path("kinds[f.kind].rust", 1).unwrap();
        assert_eq!(path.to_string(), "kinds[f.kind].rust");
        assert_eq!(path.written_upto(1), "kinds[f.kind]");
    }

    #[test]
    fn text_that_is_not_a_path_is_none() {
        assert!(parse_path("a b", 1).is_none());
        assert!(parse_path("kinds[f.kind", 1).is_none());
        assert!(parse_path("", 1).is_none());
    }

    #[test]
    fn a_lookup_by_key_reads_the_row() {
        let bindings = Bindings::from_pairs(&[("k", "int")]).with(
            "kinds",
            Datum::Record(BTreeMap::from([(
                "int".to_string(),
                Datum::Record(BTreeMap::from([(
                    "rust".to_string(),
                    Datum::Text("i64".into()),
                )])),
            )])),
        );
        let lookup = Lookup::new(&bindings);
        let path = parse_path("kinds[k].rust", 1).unwrap();
        assert_eq!(lookup.resolve(&path, "t"), Ok(&Datum::Text("i64".into())));
        let missing = parse_path("kinds[k].sql", 1).unwrap();
        assert_eq!(
            lookup.resolve(&missing, "t").unwrap_err(),
            "`kinds[k].sql` (line 1): `kinds[k]` has no `sql` (it has rust)"
        );
    }
}
