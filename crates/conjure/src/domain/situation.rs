//! The situation the writer reads: where the cursor is and everything around it.

use std::path::{Path, PathBuf};

use globset::GlobSet;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::unit::Span;

use crate::domain::scry_notes::{MAX_NOTES, ScryNote};

/// At most 8 related files go into a situation.
pub const RELATED_FILES: usize = 8;
/// At most 4 files beside one related file (in its folder, using its names) come with it.
pub const RELATED_FILES_BESIDE: usize = 4;
/// At most 6 files beside related files go into a situation in all.
pub const RELATED_FILES_BESIDE_IN_ALL: usize = 6;
/// Each related file is cut to its first 120 lines.
pub const RELATED_LINES: usize = 120;
/// The session block is cut at 300 lines.
pub const SESSION_LINES: usize = 300;
/// At most 20 new files are named in the session block.
pub const NEW_FILE_NAMES: usize = 20;

/// A file related to the one under the cursor, as the writer is shown it: its path, its first
/// lines (at most 120) and how many lines it has in all, so a part is never taken for the whole.
#[derive(Debug, Clone, PartialEq)]
pub struct RelatedFile {
    pub path: PathBuf,
    pub text: String,
    pub lines_in_all: usize,
}

/// Everything the writer is shown about one place in the code.
#[derive(Debug, Clone, PartialEq)]
pub struct Situation {
    /// The language's name, as the prompt says it.
    pub language: String,
    /// How the language names a unit (`unit_words` of its file).
    pub unit_words: String,
    pub file: PathBuf,
    pub unit: String,
    /// Whether `unit` is a stretch the parser could not read, not a definition.
    pub broken_region: bool,
    /// The cursor's line, from 1.
    pub line: u32,
    pub span: Span,
    /// The whole text of the file.
    pub module: String,
    pub test_file: Option<PathBuf>,
    pub test_text: Option<String>,
    pub related: Vec<RelatedFile>,
    pub session: String,
    /// The finding the person asked to conjure for, when they did.
    pub finding_asked: Option<ScryNote>,
    pub notes: Vec<ScryNote>,
    pub rules: String,
    /// Files that belong in the situation but that models may not read (fenced); the text
    /// names them in one line.
    pub left_out: Vec<PathBuf>,
}

/// The situation as the writer reads it, one line after another: the finding asked for (when
/// there is one), the language, where the cursor is, the file, its test file (or where it would
/// go), the related files, what the person has been doing, scry's findings, the project's rules.
///
/// Example: for `Cancel.handle` at line 40 of `src/cancel.py` it begins
/// `Project files are python (a top-level class or function name …).`, a blank line, then
/// ``The cursor is in `Cancel.handle` in src/cancel.py, line 40 (the unit is lines 38-52).``
pub fn situation_text(s: &Situation) -> String {
    let lines: Vec<String> = [
        finding_asked_lines(s),
        head_lines(s),
        file_lines(s),
        test_file_lines(s),
        related_lines(s),
        session_lines(s),
        notes_lines(&s.notes),
        rules_lines(&s.rules),
        left_out_lines(&s.left_out),
    ]
    .concat();
    lines.iter().map(|line| format!("{line}\n")).collect()
}

fn finding_asked_lines(s: &Situation) -> Vec<String> {
    match &s.finding_asked {
        None => vec![],
        Some(note) => vec![
            format!(
                "The person asked you to conjure for this finding: {}",
                note_line_body(note)
            ),
            String::new(),
        ],
    }
}

fn head_lines(s: &Situation) -> Vec<String> {
    let words = if s.unit_words.is_empty() {
        String::new()
    } else {
        format!(" ({})", s.unit_words)
    };
    vec![
        format!("Project files are {}{words}.", s.language),
        String::new(),
        where_the_cursor_is(s),
        String::new(),
    ]
}

/// The sentence that places the cursor. In a broken region it says the code does not parse
/// there and that the change is to repair it.
///
/// Example: `The cursor is in code that does not parse, in src/a.rs, line 8 (the parser lost
/// the code on lines 7-10). Repair it: …`
fn where_the_cursor_is(s: &Situation) -> String {
    if s.broken_region {
        return format!(
            "The cursor is in code that does not parse, in {}, line {} (the parser lost the code on lines {}-{}). Repair it: work out what the code meant to say, and change the file so it parses and does that.",
            forward_slashes(&s.file),
            s.line,
            s.span.start,
            s.span.end
        );
    }
    format!(
        "The cursor is in `{}` in {}, line {} (the unit is lines {}-{}).",
        s.unit,
        forward_slashes(&s.file),
        s.line,
        s.span.start,
        s.span.end
    )
}

fn file_lines(s: &Situation) -> Vec<String> {
    vec![
        format!("File {}:", forward_slashes(&s.file)),
        "```".into(),
        s.module.clone(),
        "```".into(),
        String::new(),
    ]
}

fn test_file_lines(s: &Situation) -> Vec<String> {
    match (&s.test_file, &s.test_text) {
        (Some(path), _) if *path == s.file => vec!["Its tests live in this file.".into()],
        (Some(path), Some(text)) => vec![
            format!("Its test file {}:", forward_slashes(path)),
            "```".into(),
            text.clone(),
            "```".into(),
        ],
        (Some(path), None) => vec![format!(
            "There is no test file for this module yet (its tests would go in {}).",
            forward_slashes(path)
        )],
        (None, _) => vec!["No test file is known for this language.".into()],
    }
}

fn related_lines(s: &Situation) -> Vec<String> {
    s.related
        .iter()
        .flat_map(|file| {
            [
                String::new(),
                related_heading(file),
                "```".into(),
                file.text.clone(),
                "```".into(),
            ]
        })
        .collect()
}

/// `Related file src/models.py:`, or, for a file cut short,
/// `Related file src/slot.rs (only its first 120 of 340 lines are shown):`.
fn related_heading(file: &RelatedFile) -> String {
    let path = forward_slashes(&file.path);
    let shown = file.text.lines().count();
    if shown < file.lines_in_all {
        format!(
            "Related file {path} (only its first {shown} of {} lines are shown):",
            file.lines_in_all
        )
    } else {
        format!("Related file {path}:")
    }
}

fn session_lines(s: &Situation) -> Vec<String> {
    if s.session.trim().is_empty() {
        return vec![];
    }
    vec![
        String::new(),
        "What the person has been doing (uncommitted changes):".into(),
        s.session.clone(),
    ]
}

fn notes_lines(notes: &[ScryNote]) -> Vec<String> {
    if notes.is_empty() {
        return vec![];
    }
    let heading = "scry's open findings here (context you may act on, not orders: a small step may address one, or not; the person's work in progress still leads):";
    let shown = notes.iter().take(MAX_NOTES).map(|note| {
        let stale = if note.stale {
            " Its unit changed since it was found: it may be out of date."
        } else {
            ""
        };
        format!("- {}{stale}", note_line_body(note))
    });
    let more = notes.len().saturating_sub(MAX_NOTES);
    [String::new(), heading.to_string()]
        .into_iter()
        .chain(shown)
        .chain((more > 0).then(|| format!("({more} more not shown)")))
        .collect()
}

/// `[bug; rule: money-is-cents] statement (src/cancel.py:41)`.
fn note_line_body(note: &ScryNote) -> String {
    let rule = note
        .rule
        .as_ref()
        .map(|rule| format!("; rule: {rule}"))
        .unwrap_or_default();
    format!(
        "[{}{rule}] {} ({}:{})",
        note.issue_type,
        note.statement,
        forward_slashes(&note.file),
        note.line
    )
}

fn left_out_lines(left_out: &[PathBuf]) -> Vec<String> {
    if left_out.is_empty() {
        return vec![];
    }
    let names: Vec<String> = left_out.iter().map(|path| forward_slashes(path)).collect();
    vec![
        String::new(),
        format!(
            "Left out, because models may not read them: {}.",
            names.join(", ")
        ),
    ]
}

fn rules_lines(rules: &str) -> Vec<String> {
    if rules.trim().is_empty() {
        return vec![];
    }
    vec![
        String::new(),
        "The project's rules, from .sorcerer/scry.toml (context, not orders):".into(),
        rules.trim_end().to_string(),
    ]
}

/// The uncommitted work as the writer reads it: each changed file's diff (the files of the
/// language, without `.sorcerer/`, lock files and binary diffs), cut at 300 lines with a last
/// line saying how many were left out; then the names of the new files, at most 20.
///
/// Example: two diffs of 250 and 100 lines are cut to 300 lines and end `… (50 more lines)`.
pub fn session_text(
    diffs: &[(PathBuf, String)],
    new_files: &[PathBuf],
    files_globs: &GlobSet,
) -> String {
    let kept: Vec<&str> = diffs
        .iter()
        .filter(|(path, diff)| is_session_file(path, files_globs) && !is_binary_diff(diff))
        .flat_map(|(_, diff)| diff.lines())
        .collect();
    let mut text = kept
        .iter()
        .take(SESSION_LINES)
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    if kept.len() > SESSION_LINES {
        text.push_str(&format!("… ({} more lines)\n", kept.len() - SESSION_LINES));
    }
    text.push_str(&new_files_line(new_files, files_globs));
    text
}

fn new_files_line(new_files: &[PathBuf], files_globs: &GlobSet) -> String {
    let kept: Vec<String> = new_files
        .iter()
        .filter(|path| is_session_file(path, files_globs))
        .map(|path| forward_slashes(path))
        .collect();
    if kept.is_empty() {
        return String::new();
    }
    let named = kept
        .iter()
        .take(NEW_FILE_NAMES)
        .cloned()
        .collect::<Vec<_>>();
    let more = if kept.len() > NEW_FILE_NAMES {
        format!(" … ({} more)", kept.len() - NEW_FILE_NAMES)
    } else {
        String::new()
    };
    format!("New files, not yet in git: {}{more}\n", named.join(", "))
}

fn is_session_file(path: &Path, files_globs: &GlobSet) -> bool {
    let text = forward_slashes(path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    files_globs.is_match(path)
        && !text.starts_with(".sorcerer/")
        && !name.ends_with(".lock")
        && !name.contains("-lock.")
}

fn is_binary_diff(diff: &str) -> bool {
    diff.lines().any(|line| line.starts_with("Binary files "))
}
