//! Owned regions: a list of everything of a kind (one line per file of a folder, say), kept by
//! spellbook between two marker lines and rewritten whole from what is on disk. A region is
//! declared in `.sorcerer/spellbook/regions.toml`; this module reads the declarations, renders a
//! region's lines, and finds and rewrites the block in a file's text. No I/O.
//!
//! A block in a file whose comments start with `//` reads:
//!
//! ```text
//! // spellbook:region modules (from src/a/*.txt; edit .sorcerer/spellbook/regions.toml, not this block)
//! item one;
//! item two;
//! // spellbook:end modules
//! ```

use std::collections::BTreeMap;
use std::fmt;

use sorcerer_core::domain::language::CommentMarks;
use sorcerer_core::domain::templates::fill;

use crate::domain::anchor::Anchor;
use crate::domain::change::with_line_ending;
use crate::domain::generator_problems::unknown_key_text;
use crate::domain::path_pattern::PathPattern;
use crate::domain::plan::relative_path;
use crate::domain::rule::{Severity, patterns_of, severity_of, string_of};

/// Where the regions are declared, relative to the project root.
pub const REGIONS_FILE: &str = ".sorcerer/spellbook/regions.toml";

/// The word an opening marker carries after the comment marker.
const OPENS: &str = sorcerer_core::domain::generated_regions::REGION_OPENS;
/// The word a closing marker carries after the comment marker.
const ENDS: &str = "spellbook:end";

/// One `[regions.NAME]`: which files hold the block, what it lists, and how each line reads.
///
/// Example: `[regions.modules]` with `file = "src/{service}/{kind}/list.txt"`, `query = { glob =
/// "{= dir =}/*.txt" }` and `item = "item {= stem =};"` keeps, in every `list.txt` two folders
/// under `src/`, one `item NAME;` line per sibling `.txt` file.
#[derive(Debug, Clone)]
pub struct Region {
    pub name: String,
    /// The file(s) that hold the block; a pattern matching several files is a family, one
    /// region in each.
    pub file: PathPattern,
    /// Files `file` matches that the region leaves alone.
    pub exclude: Vec<PathPattern>,
    pub query: RegionQuery,
    /// One line per match, a template.
    pub item: String,
    /// Where `sync --init` puts the markers: phrases tried in order.
    pub init_at: Vec<String>,
    /// The marker lines' comment syntax, for a file no language covers.
    pub comment: Option<String>,
    /// How serious a stale block is.
    pub severity: Severity,
}

/// What a region lists: a glob over the project (a template over the region file's slots,
/// `{= dir =}` and `{= file =}`), less the globs in `exclude`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionQuery {
    pub glob: String,
    pub exclude: Vec<String>,
}

/// The project's regions as `regions.toml` declares them, or why it did not load. A plan keeps
/// going without regions (and says why in a note); `sync` and `check` stop.
pub type DeclaredRegions = Result<Vec<Region>, RegionsProblem>;

/// Why `regions.toml` did not load: the sentence naming the region and the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionsProblem {
    pub why: String,
}

impl fmt::Display for RegionsProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{REGIONS_FILE}: {}", self.why)
    }
}

const REGION_KEYS: &[&str] = &[
    "file", "exclude", "query", "item", "init_at", "comment", "severity",
];
const QUERY_KEYS: &[&str] = &["glob", "exclude"];
/// olifant's region keys that are not here yet.
const LATER_REGION_KEYS: &[&str] = &["whole_file", "template", "init_indent"];
/// The names a query's glob may use besides the file pattern's slots.
const QUERY_NAMES: &[&str] = &["dir", "file"];
/// The names an item may use besides the file pattern's slots.
const ITEM_NAMES: &[&str] = &["stem", "parent", "dir", "path"];

/// Reads `regions.toml`: one `[regions.NAME]` table each, in file order. An unknown key, a
/// missing `file`, `query` or `item`, a phrase that does not parse, a template over an unknown
/// name, or a `file` that leaves the project is refused with a sentence naming the region and
/// the key.
///
/// Example: `[regions.modules]` with `quary = …` gives "region modules: `quary` is not a key;
/// did you mean `query`?".
pub fn parse_regions(text: &str) -> Result<Vec<Region>, RegionsProblem> {
    let problem = |why: String| RegionsProblem { why };
    let table: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| problem(e.message().trim().to_string()))?;
    let mut regions = Vec::new();
    for (key, value) in &table {
        if key != "regions" {
            return Err(problem(format!(
                "{} (regions are `[regions.<name>]` tables)",
                unknown_key_text(key, &["regions"])
            )));
        }
        let declared = value
            .as_table()
            .ok_or_else(|| problem("`regions` is a table of `[regions.<name>]`".into()))?;
        for (name, body) in declared {
            let region =
                parse_region(name, body).map_err(|why| problem(format!("region {name}: {why}")))?;
            regions.push(region);
        }
    }
    Ok(regions)
}

fn parse_region(name: &str, value: &toml::Value) -> Result<Region, String> {
    if !is_a_region_name(name) {
        return Err(
            "a region's name is one word or kebab-case words (`modules`, `commands-exports`)"
                .into(),
        );
    }
    let table = value
        .as_table()
        .ok_or("it is a table, `[regions.<name>]`")?;
    refuse_unknown_keys(table)?;
    let file = region_file(table)?;
    let query = query_of(table)?;
    let item =
        string_of(table, "item")?.ok_or("`item` is missing: the line written for each match")?;
    let region = Region {
        name: name.to_string(),
        exclude: patterns_of(table, "exclude")?,
        init_at: init_at_of(table)?,
        comment: string_of(table, "comment")?,
        severity: severity_of(table)?,
        file,
        query,
        item,
    };
    refuse_unknown_names(&region)?;
    Ok(region)
}

fn is_a_region_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn refuse_unknown_keys(table: &toml::Table) -> Result<(), String> {
    for key in table.keys() {
        if LATER_REGION_KEYS.contains(&key.as_str()) {
            return Err(format!(
                "`{key}` is not in spellbook yet (a later milestone)"
            ));
        }
        if !REGION_KEYS.contains(&key.as_str()) {
            return Err(unknown_key_text(key, REGION_KEYS));
        }
    }
    Ok(())
}

/// `file`: a path pattern inside the project, off spellbook's own folder and git's.
fn region_file(table: &toml::Table) -> Result<PathPattern, String> {
    let text = string_of(table, "file")?
        .ok_or("`file` is missing: the path pattern of the file(s) that hold the block")?;
    let kept = relative_path(&text).map_err(|why| format!("file: `{text}`: {why}"))?;
    if kept.starts_with(".sorcerer/") || kept.starts_with(".git/") {
        return Err(format!(
            "file: `{text}` is a fenced path (.sorcerer/ and .git/ are never written)"
        ));
    }
    PathPattern::parse(&kept).map_err(|why| format!("file: {why}"))
}

fn query_of(table: &toml::Table) -> Result<RegionQuery, String> {
    let shape = "`query` is a table, `{ glob = \"…\", exclude = [\"…\"] }`";
    let value = table
        .get("query")
        .ok_or("`query` is missing: `{ glob = \"…\" }`, what the region lists")?;
    let query = value.as_table().ok_or(shape)?;
    for key in query.keys() {
        if !QUERY_KEYS.contains(&key.as_str()) {
            return Err(format!("query: {}", unknown_key_text(key, QUERY_KEYS)));
        }
    }
    let glob = string_of(query, "glob")
        .map_err(|why| format!("query: {why}"))?
        .ok_or("query: `glob` is missing")?;
    let exclude = match query.get("exclude") {
        None => Vec::new(),
        Some(list) => list
            .as_array()
            .and_then(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().map(String::from))
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or("query: `exclude` is a list of globs")?,
    };
    Ok(RegionQuery { glob, exclude })
}

/// `init_at`: a phrase or a list of them, each one an insert's `at` would take.
fn init_at_of(table: &toml::Table) -> Result<Vec<String>, String> {
    let phrases = match table.get("init_at") {
        None => vec!["end of imports".to_string()],
        Some(toml::Value::String(phrase)) => vec![phrase.clone()],
        Some(toml::Value::Array(list)) => list
            .iter()
            .map(|phrase| phrase.as_str().map(String::from))
            .collect::<Option<Vec<_>>>()
            .ok_or("`init_at` is a phrase or a list of phrases")?,
        Some(_) => return Err("`init_at` is a phrase or a list of phrases".into()),
    };
    for phrase in &phrases {
        Anchor::parse(phrase).map_err(|why| format!("init_at: {why}"))?;
    }
    Ok(phrases)
}

/// Every name the query's globs and the item use is one they may use.
fn refuse_unknown_names(region: &Region) -> Result<(), String> {
    let slots: Vec<&str> = region.file.slots().iter().map(String::as_str).collect();
    let dry_fill = |template: &str, extra: &[&str]| {
        let values: Vec<(&str, &str)> =
            slots.iter().chain(extra).map(|name| (*name, "x")).collect();
        fill(template, &values).map(|_| ())
    };
    dry_fill(&region.query.glob, QUERY_NAMES).map_err(|why| format!("query: {why}"))?;
    for glob in &region.query.exclude {
        dry_fill(glob, QUERY_NAMES).map_err(|why| format!("query: {why}"))?;
    }
    dry_fill(&region.item, ITEM_NAMES).map_err(|why| format!("item: {why}"))
}

/// A region laid on one file `file` matches: the file and the slots it bound.
///
/// Example: `src/{service}/{kind}/list.txt` on `src/booking/commands/list.txt` binds `service`
/// to `booking` and `kind` to `commands`.
#[derive(Debug, Clone)]
pub struct RegionInstance<'r> {
    pub region: &'r Region,
    /// Relative to the root, with `/`.
    pub file: String,
    pub slots: BTreeMap<String, String>,
}

impl Region {
    /// The region in this file, when `file` matches it and `exclude` does not.
    pub fn instance_in(&self, file: &str) -> Option<RegionInstance<'_>> {
        let slots = self.file.matches(file)?;
        if self
            .exclude
            .iter()
            .any(|pattern| pattern.matches(file).is_some())
        {
            return None;
        }
        Some(RegionInstance {
            region: self,
            file: file.to_string(),
            slots,
        })
    }
}

impl RegionInstance<'_> {
    /// The query's glob for this file: the slots, `{= dir =}` (the file's folder) and
    /// `{= file =}` filled.
    ///
    /// Example: `{= dir =}/*.txt` for `src/a/list.txt` is `src/a/*.txt`.
    pub fn query_glob(&self) -> Result<String, String> {
        fill(&self.region.query.glob, &self.query_values())
    }

    /// The query's excludes for this file, filled as the glob is.
    pub fn query_excludes(&self) -> Result<Vec<String>, String> {
        let values = self.query_values();
        self.region
            .query
            .exclude
            .iter()
            .map(|glob| fill(glob, &values))
            .collect()
    }

    fn query_values(&self) -> Vec<(&str, &str)> {
        let mut values: Vec<(&str, &str)> = self
            .slots
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        values.push(("dir", folder_of(&self.file)));
        values.push(("file", self.file.as_str()));
        values
    }

    /// The block's lines for the files the query matched: the item rendered for each (its
    /// `stem`, `parent`, `dir`, `path` and the slots), trimmed, empty ones dropped, each once,
    /// sorted by bytes so every platform sorts alike.
    ///
    /// Example: `item {= stem =};` over `src/a/b.txt` and `src/a/a.txt` is `["item a;", "item
    /// b;"]`.
    pub fn items(&self, matches: &[String]) -> Result<Vec<String>, String> {
        let mut lines = matches
            .iter()
            .map(|path| self.item_for(path))
            .collect::<Result<Vec<_>, _>>()?;
        lines.retain(|line| !line.is_empty());
        lines.sort();
        lines.dedup();
        Ok(lines)
    }

    fn item_for(&self, path: &str) -> Result<String, String> {
        let dir = folder_of(path);
        let name = path.rsplit('/').next().unwrap_or(path);
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        let parent = dir.rsplit('/').next().unwrap_or(dir);
        let mut values: Vec<(&str, &str)> = self
            .slots
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        values.extend([
            ("stem", stem),
            ("parent", parent),
            ("dir", dir),
            ("path", path),
        ]);
        fill(&self.region.item, &values).map(|line| line.trim().to_string())
    }

    /// The opening marker, in the file's comment syntax, naming the glob it lists.
    ///
    /// Example: `// spellbook:region modules (from src/a/*.txt; edit
    /// .sorcerer/spellbook/regions.toml, not this block)`.
    /// In a language whose comments close, the marker closes too: `<!-- spellbook:region … -->`.
    pub fn opening_marker(&self, comment: &CommentMarks, glob: &str) -> String {
        comment.around(&format!(
            "{OPENS} {} (from {glob}; edit {REGIONS_FILE}, not this block)",
            self.region.name
        ))
    }

    /// The closing marker: `// spellbook:end modules`, `<!-- spellbook:end modules -->`.
    pub fn closing_marker(&self, comment: &CommentMarks) -> String {
        comment.around(&format!("{ENDS} {}", self.region.name))
    }
}

/// The folder part of a path, empty for a file at the root.
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Where a region's block is in a file: the rows (from 0) of its two markers, and the opening
/// marker's indentation, which every line of the block takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub open_row: usize,
    pub close_row: usize,
    pub indent: String,
}

/// A marker pair that is not one: the row (from 0) of the first marker found, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokenMarkers {
    pub row: usize,
    pub why: String,
}

/// The block of the region `name` in a text whose comments are written with `comment`: `None`
/// when the file has no markers for it. A line is a marker when it is one whole comment whose
/// text is `spellbook:region NAME` (anything may follow) or `spellbook:end NAME`. An opening
/// without its end, two openings, or an end before its start is broken.
///
/// Example: in `a\n// spellbook:region m\nx\n// spellbook:end m\n` the block of `m` is rows 1
/// to 3.
pub fn find_block(
    text: &str,
    name: &str,
    comment: &CommentMarks,
) -> Result<Option<Block>, BrokenMarkers> {
    let mut opening: Option<(usize, String)> = None;
    let mut block: Option<Block> = None;
    for (row, line) in text.lines().enumerate() {
        match marker_of(line, comment, name) {
            None => {}
            Some(Marker::Opens) => {
                if let Some((first, _)) = &opening {
                    return Err(broken(*first, opens_twice_text(name, *first, row)));
                }
                if let Some(found) = &block {
                    return Err(broken(
                        found.open_row,
                        opens_twice_text(name, found.open_row, row),
                    ));
                }
                opening = Some((row, indent_of(line)));
            }
            Some(Marker::Ends) => match opening.take() {
                Some((open_row, indent)) => {
                    block = Some(Block {
                        open_row,
                        close_row: row,
                        indent,
                    });
                }
                None => {
                    return Err(broken(
                        row,
                        format!("region {name} ends on line {} before it opens", row + 1),
                    ));
                }
            },
        }
    }
    match opening {
        Some((row, _)) => Err(broken(
            row,
            format!(
                "region {name} opens on line {} and never ends (no `{}`)",
                row + 1,
                comment.around(&format!("{ENDS} {name}"))
            ),
        )),
        None => Ok(block),
    }
}

enum Marker {
    Opens,
    Ends,
}

fn marker_of(line: &str, comment: &CommentMarks, name: &str) -> Option<Marker> {
    let after_comment = comment.inside(line)?;
    let (marker, rest) = if let Some(rest) = after_comment.strip_prefix(OPENS) {
        (Marker::Opens, rest)
    } else {
        (Marker::Ends, after_comment.strip_prefix(ENDS)?)
    };
    let rest = rest.strip_prefix([' ', '\t'])?.trim_start();
    let named = rest.strip_prefix(name)?;
    let name_ends = named.is_empty() || named.starts_with([' ', '\t', '\r']);
    name_ends.then_some(marker)
}

fn broken(row: usize, why: String) -> BrokenMarkers {
    BrokenMarkers { row, why }
}

fn opens_twice_text(name: &str, first: usize, second: usize) -> String {
    format!(
        "region {name} opens twice (lines {} and {})",
        first + 1,
        second + 1
    )
}

fn indent_of(line: &str) -> String {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// The text with the block's lines replaced: the opening marker, the items, the closing
/// marker, each with the block's indentation, in the file's own line ending. Nothing outside
/// the markers changes.
///
/// Example: the block `// spellbook:region m\nold\n// spellbook:end m\n` with the items `a`
/// and `b` becomes `// spellbook:region m (from …)\na\nb\n// spellbook:end m\n`.
pub fn splice(text: &str, block: &Block, markers: &Markers, items: &[String]) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let indented = |line: &str| format!("{}{line}\n", block.indent);
    let mut middle = indented(&markers.opening);
    for item in items {
        middle.push_str(&indented(item));
    }
    middle.push_str(&indented(&markers.closing));
    let closing_had_no_ending = lines
        .get(block.close_row)
        .is_some_and(|line| !line.ends_with('\n'));
    if closing_had_no_ending {
        middle.pop();
    }
    let mut spliced: String = lines[..block.open_row].concat();
    spliced.push_str(&with_line_ending(&middle, text));
    spliced.push_str(&lines[block.close_row + 1..].concat());
    spliced
}

/// The two marker lines a block is written between.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Markers {
    pub opening: String,
    pub closing: String,
}

/// The text with an empty block put before `row` (from 0; the row count is the end of the
/// file), indented by `indent`: what `sync --init` starts from.
///
/// Example: `a\nb\n` with the markers at row 1 is `a\n// …region m…\n// …end m\nb\n`.
pub fn with_markers_at(text: &str, row: usize, indent: &str, markers: &Markers) -> String {
    let mut lines: Vec<String> = text.split_inclusive('\n').map(String::from).collect();
    let at_the_end = row >= lines.len();
    if let Some(last) = lines.last_mut()
        && at_the_end
        && !last.ends_with('\n')
    {
        last.push('\n');
    }
    let pair = format!("{indent}{}\n{indent}{}\n", markers.opening, markers.closing);
    let at = row.min(lines.len());
    lines.insert(at, with_line_ending(&pair, text));
    lines.concat()
}

/// The row new markers go before so that they sit inside no block: a row inside a block
/// (after its opening marker, up to its closing marker) becomes the row just after the block's
/// closing marker; any other row stays. `init_at` phrases know lines, not blocks, and the last
/// line matching a pattern is often the last item of a block set up already.
///
/// Example: with a block on rows 2 to 5, row 4 becomes 6, and rows 2 and 6 stay as they are.
pub fn outside_every_block(row: usize, blocks: &[Block]) -> usize {
    match blocks
        .iter()
        .find(|block| block.open_row < row && row <= block.close_row)
    {
        Some(block) => outside_every_block(block.close_row + 1, blocks),
        None => row,
    }
}

/// The text without the lines outside every block whose trimmed text is one of the items: on
/// `sync --init`, a list written by hand moves into the new block instead of being declared
/// twice. `blocks` are every block in the text, the new one among them; a line inside any of
/// them belongs to its region and stays.
///
/// Example: `item a;\n<block>\n` with the item `item a;` loses its first line.
pub fn adopt(text: &str, blocks: &[Block], items: &[String]) -> String {
    text.split_inclusive('\n')
        .enumerate()
        .filter(|(row, line)| {
            let inside = blocks
                .iter()
                .any(|block| (block.open_row..=block.close_row).contains(row));
            inside || !items.iter().any(|item| item == line.trim())
        })
        .map(|(_, line)| line)
        .collect()
}

/// A line a rewrite would lose: its row (from 0) in the text before, and its text, trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LostLine {
    pub row: usize,
    pub text: String,
}

/// The first line of `before` that `after` no longer has, when the only lines a sync may
/// remove are a block's items (its region owns them) and the hand-written lines it adopts (a
/// line outside the blocks before whose text is inside a block after). Lines are compared
/// trimmed, so an adopted line may take the block's indentation; a line written twice must be
/// there twice, unless a block holds it now. Every marker line is kept, so a block that would
/// swallow another region's block loses that block's markers and is refused. The one exception
/// is an opening marker's wording: sync rewrites it (a bare `// spellbook:region m` becomes the
/// full line), which is allowed when `after` has as many blocks as `before`.
///
/// Example: before `a\n// spellbook:region m\nx\n// spellbook:end m\nb\n` (the block on rows
/// 1 to 3), an after without `x` loses nothing, one without `b` loses row 4 (`b`).
pub fn first_lost_line(
    before: &str,
    blocks_before: &[Block],
    after: &str,
    blocks_after: &[Block],
) -> Option<LostLine> {
    let owned = |blocks: &[Block], row: usize| {
        let a_marker = blocks
            .iter()
            .any(|block| row == block.open_row || row == block.close_row);
        let inside = blocks
            .iter()
            .any(|block| block.open_row < row && row < block.close_row);
        inside && !a_marker
    };
    let held_by_a_block: Vec<&str> = after
        .lines()
        .enumerate()
        .filter(|(row, _)| owned(blocks_after, *row))
        .map(|(_, line)| line.trim())
        .collect();
    let mut left: BTreeMap<&str, usize> = BTreeMap::new();
    for line in after.lines() {
        *left.entry(line.trim()).or_default() += 1;
    }
    let reworded_openings: Vec<usize> = if blocks_before.len() == blocks_after.len() {
        blocks_before.iter().map(|block| block.open_row).collect()
    } else {
        Vec::new()
    };
    before
        .lines()
        .enumerate()
        .filter(|(row, _)| !owned(blocks_before, *row) && !reworded_openings.contains(row))
        .find_map(|(row, line)| {
            let text = line.trim();
            let still_there = match left.get_mut(text) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    true
                }
                _ => held_by_a_block.contains(&text),
            };
            (!still_there).then(|| LostLine {
                row,
                text: text.to_string(),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DECLARED: &str = r#"
[regions.modules]
file    = "src/{service}/{kind}/list.txt"
exclude = ["src/*/skip/list.txt"]
query   = { glob = "{= dir =}/*.txt", exclude = ["{= dir =}/list.txt"] }
item    = "item {= stem =};"
init_at = ["after last line matching ^item ", "start of file"]
"#;

    fn declared() -> Vec<Region> {
        parse_regions(DECLARED).unwrap()
    }

    fn problem(text: &str) -> String {
        parse_regions(text).unwrap_err().why
    }

    fn markers(comment: &str) -> Markers {
        Markers {
            opening: format!("{comment} spellbook:region modules (from x)"),
            closing: format!("{comment} spellbook:end modules"),
        }
    }

    #[test]
    fn a_region_reads_its_keys() {
        let regions = declared();
        assert_eq!(regions.len(), 1);
        let region = &regions[0];
        assert_eq!(region.name, "modules");
        assert_eq!(region.file.text(), "src/{service}/{kind}/list.txt");
        assert_eq!(region.query.glob, "{= dir =}/*.txt");
        assert_eq!(region.init_at.len(), 2);
        assert_eq!(region.severity, Severity::Error);
        assert_eq!(region.comment, None);
    }

    #[test]
    fn mistakes_name_the_region_and_the_key() {
        assert_eq!(
            problem("[regions.m]\nfile = \"a.txt\"\nquary = { glob = \"*\" }\nitem = \"x\"\n"),
            "region m: `quary` is not a key; did you mean `query`?"
        );
        assert!(
            problem("[regions.m]\nfile = \"a.txt\"\nquery = { glob = \"*\" }\n")
                .starts_with("region m: `item` is missing")
        );
        assert!(
            problem("[regions.m]\nfile = \"a.txt\"\nitem = \"x\"\n")
                .starts_with("region m: `query` is missing")
        );
        assert!(
            problem("[regions.m]\nfile = \"../a.txt\"\nquery = { glob = \"*\" }\nitem = \"x\"\n")
                .contains("leaves the project")
        );
        assert!(
            problem(
                "[regions.m]\nfile = \".sorcerer/a.txt\"\nquery = { glob = \"*\" }\nitem = \"x\"\n"
            )
            .contains("fenced path")
        );
        assert!(problem(
            "[regions.m]\nfile = \"a.txt\"\nquery = { glob = \"*\" }\nitem = \"x\"\ninit_at = \"end of cases\"\n"
        )
        .starts_with("region m: init_at: `end of cases`"));
        assert!(
            problem(
                "[regions.m]\nfile = \"a.txt\"\nquery = { glob = \"*\" }\nitem = \"{= kind =}\"\n"
            )
            .starts_with("region m: item: `kind` is not a name")
        );
        assert!(problem(
            "[regions.m]\nfile = \"a.txt\"\nquery = { glob = \"*\" }\nitem = \"x\"\nwhole_file = true\n"
        )
        .contains("not in spellbook yet"));
    }

    #[test]
    fn a_family_is_one_region_in_each_file() {
        let regions = declared();
        let region = &regions[0];
        let instance = region.instance_in("src/booking/commands/list.txt").unwrap();
        assert_eq!(instance.slots["kind"], "commands");
        assert_eq!(instance.query_glob().unwrap(), "src/booking/commands/*.txt");
        assert_eq!(
            instance.query_excludes().unwrap(),
            vec!["src/booking/commands/list.txt".to_string()]
        );
        assert!(region.instance_in("src/booking/skip/list.txt").is_none());
        assert!(region.instance_in("src/booking/list.txt").is_none());
    }

    #[test]
    fn in_a_language_whose_comments_close_the_markers_close_too() {
        let marks = CommentMarks {
            start: "<!--".into(),
            end: Some("-->".into()),
        };
        let regions = declared();
        let instance = regions[0]
            .instance_in("src/booking/commands/list.txt")
            .unwrap();
        let opening = instance.opening_marker(&marks, "src/x/*.txt");
        let closing = instance.closing_marker(&marks);
        assert!(opening.starts_with("<!-- spellbook:region ") && opening.ends_with(" -->"));
        assert!(closing.starts_with("<!-- spellbook:end ") && closing.ends_with(" -->"));
        let text = format!("a\n{opening}\nitem\n{closing}\nb\n");
        let block = find_block(&text, &instance.region.name, &marks)
            .unwrap()
            .unwrap();
        assert_eq!((block.open_row, block.close_row), (1, 3));
        let never_ends = find_block("<!-- spellbook:region m -->\n", "m", &marks).unwrap_err();
        assert!(
            never_ends.why.ends_with("(no `<!-- spellbook:end m -->`)"),
            "{}",
            never_ends.why
        );
    }

    #[test]
    fn items_are_rendered_sorted_and_once() {
        let regions = declared();
        let instance = regions[0].instance_in("src/a/b/list.txt").unwrap();
        let matches = [
            "src/a/b/zeta.txt".to_string(),
            "src/a/b/alpha.txt".to_string(),
            "src/a/b/alpha.txt".to_string(),
        ];
        assert_eq!(
            instance.items(&matches).unwrap(),
            vec!["item alpha;".to_string(), "item zeta;".to_string()]
        );
    }

    #[test]
    fn an_item_may_name_the_parent_folder_and_filter_it() {
        let region = &parse_regions(
            "[regions.p]\nfile = \"a/list.txt\"\nquery = { glob = \"{= dir =}/*/x.txt\" }\nitem = \"{= parent|pascal =}\"\n",
        )
        .unwrap()[0];
        let instance = region.instance_in("a/list.txt").unwrap();
        assert_eq!(
            instance.items(&["a/clock_port/x.txt".to_string()]).unwrap(),
            vec!["ClockPort".to_string()]
        );
    }

    #[test]
    fn the_block_is_found_by_its_markers() {
        let text =
            "a\n  # spellbook:region modules (from x)\n  old\n  # spellbook:end modules\nb\n";
        assert_eq!(
            find_block(text, "modules", &CommentMarks::line("#")).unwrap(),
            Some(Block {
                open_row: 1,
                close_row: 3,
                indent: "  ".into()
            })
        );
        assert_eq!(
            find_block(text, "other", &CommentMarks::line("#")).unwrap(),
            None
        );
        assert_eq!(
            find_block(
                "# spellbook:region modules-two\n",
                "modules",
                &CommentMarks::line("#")
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn broken_markers_say_the_line() {
        let never_ends =
            find_block("a\n# spellbook:region m\n", "m", &CommentMarks::line("#")).unwrap_err();
        assert_eq!(never_ends.row, 1);
        assert_eq!(
            never_ends.why,
            "region m opens on line 2 and never ends (no `# spellbook:end m`)"
        );
        let twice =
            "# spellbook:region m\n# spellbook:end m\n# spellbook:region m\n# spellbook:end m\n";
        assert_eq!(
            find_block(twice, "m", &CommentMarks::line("#"))
                .unwrap_err()
                .why,
            "region m opens twice (lines 1 and 3)"
        );
        assert_eq!(
            find_block("# spellbook:end m\n", "m", &CommentMarks::line("#"))
                .unwrap_err()
                .why,
            "region m ends on line 1 before it opens"
        );
    }

    #[test]
    fn a_splice_rewrites_only_the_block() {
        let text = "head\n// spellbook:region modules\nhand edit\n// spellbook:end modules\ntail\n";
        let block = find_block(text, "modules", &CommentMarks::line("//"))
            .unwrap()
            .unwrap();
        let items = vec!["item a;".to_string(), "item b;".to_string()];
        assert_eq!(
            splice(text, &block, &markers("//"), &items),
            "head\n// spellbook:region modules (from x)\nitem a;\nitem b;\n// spellbook:end modules\ntail\n"
        );
    }

    #[test]
    fn a_splice_keeps_the_files_line_ending() {
        let text = "head\r\n# spellbook:region modules\r\n# spellbook:end modules\r\n";
        let block = find_block(text, "modules", &CommentMarks::line("#"))
            .unwrap()
            .unwrap();
        assert_eq!(
            splice(text, &block, &markers("#"), &["x".to_string()]),
            "head\r\n# spellbook:region modules (from x)\r\nx\r\n# spellbook:end modules\r\n"
        );
    }

    #[test]
    fn init_puts_the_markers_and_adopts_the_hand_lines() {
        let text = "//! doc\n\nitem a;\n";
        let marked = with_markers_at(text, 3, "", &markers("//"));
        assert_eq!(
            marked,
            "//! doc\n\nitem a;\n// spellbook:region modules (from x)\n// spellbook:end modules\n"
        );
        let block = find_block(&marked, "modules", &CommentMarks::line("//"))
            .unwrap()
            .unwrap();
        let adopted = adopt(
            &marked,
            std::slice::from_ref(&block),
            &["item a;".to_string()],
        );
        assert_eq!(
            adopted,
            "//! doc\n\n// spellbook:region modules (from x)\n// spellbook:end modules\n"
        );
    }

    #[test]
    fn markers_go_into_an_empty_file() {
        assert_eq!(
            with_markers_at("", 0, "", &markers("#")),
            "# spellbook:region modules (from x)\n# spellbook:end modules\n"
        );
        assert_eq!(
            with_markers_at("a", 1, "", &markers("#")),
            "a\n# spellbook:region modules (from x)\n# spellbook:end modules\n"
        );
    }

    fn block(open_row: usize, close_row: usize) -> Block {
        Block {
            open_row,
            close_row,
            indent: String::new(),
        }
    }

    #[test]
    fn a_row_inside_a_block_moves_past_its_closing_marker() {
        let blocks = [block(2, 5), block(6, 8)];
        assert_eq!(outside_every_block(4, &blocks), 6);
        assert_eq!(outside_every_block(5, &blocks), 6);
        assert_eq!(outside_every_block(7, &blocks), 9);
        assert_eq!(outside_every_block(2, &blocks), 2);
        assert_eq!(outside_every_block(1, &blocks), 1);
    }

    #[test]
    fn adopting_leaves_the_lines_of_other_blocks_alone() {
        let text = "item a;\n# region n\nitem a;\n# end n\n# region m\n# end m\n";
        let adopted = adopt(text, &[block(1, 3), block(4, 5)], &["item a;".into()]);
        assert_eq!(
            adopted,
            "# region n\nitem a;\n# end n\n# region m\n# end m\n"
        );
    }

    #[test]
    fn a_line_outside_the_blocks_must_survive_or_be_adopted() {
        let before = "a\n# region m\nx\n# end m\nb\nitem c;\n";
        let blocks = [block(1, 3)];
        let no_x = "a\n# region m\n# end m\nb\nitem c;\n";
        assert_eq!(first_lost_line(before, &blocks, no_x, &[block(1, 2)]), None);
        let adopted = "a\n# region m\n  item c;\n# end m\nb\n";
        assert_eq!(
            first_lost_line(before, &blocks, adopted, &[block(1, 3)]),
            None
        );
        let no_b = "a\n# region m\nx\n# end m\nitem c;\n";
        assert_eq!(
            first_lost_line(before, &blocks, no_b, &[block(1, 3)]),
            Some(LostLine {
                row: 4,
                text: "b".into()
            })
        );
    }

    #[test]
    fn a_rewritten_opening_marker_is_not_a_lost_line() {
        let before = "# spellbook:region m\nx\n# spellbook:end m\n";
        let after = "# spellbook:region m (from a/*.txt; edit r.toml, not this block)\nx\n# spellbook:end m\n";
        assert_eq!(
            first_lost_line(before, &[block(0, 2)], after, &[block(0, 2)]),
            None
        );
    }

    #[test]
    fn a_block_that_swallows_another_loses_its_markers() {
        let before = "# region m\nx\n# region n\ny\n# end n\n# end m\n";
        let after = "# region m\nx\n# end m\n";
        assert_eq!(
            first_lost_line(before, &[block(0, 5), block(2, 4)], after, &[block(0, 2)]),
            Some(LostLine {
                row: 2,
                text: "# region n".into()
            })
        );
    }
}
