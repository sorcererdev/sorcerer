//! The marks a landed change shows: one Information diagnostic on the first changed line of
//! each file, from source `conjure`, carrying the change's id, so the editor hands the id back
//! when the person asks for the actions on the line. The words are built last in the file.

use std::path::PathBuf;

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use serde_json::Value;

use super::changes::{LandedChange, LandedFile};

/// One mark: which change, in which file (as the editor names it), on which line (from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Mark {
    pub change_id: String,
    pub path: PathBuf,
    pub line: u32,
    pub kind: MarkKind,
}

/// What a mark says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkKind {
    /// On the file the change was asked in: the goal, the model, the cost, the time, whether
    /// it parses, the other files.
    Main,
    /// On each other changed file: which file the change was asked in, and its goal.
    Other,
    /// On the unit's first line when the writer changed nothing.
    NoChange,
}

/// The marks of a change: the main one on its main file, one on each other file; a change of
/// no files has one mark, on the first line of the unit it was asked in.
pub(super) fn marks_of(change: &LandedChange) -> Vec<Mark> {
    let mark = |path: &PathBuf, line, kind| Mark {
        change_id: change.id.clone(),
        path: path.clone(),
        line,
        kind,
    };
    let Some(main) = change.main_file() else {
        return vec![mark(
            &change.asked_path,
            change.unit_line,
            MarkKind::NoChange,
        )];
    };
    let others = change
        .other_files(main)
        .map(|file| mark(&file.editor_path, file.first_changed_line, MarkKind::Other));
    std::iter::once(mark(
        &main.editor_path,
        main.first_changed_line,
        MarkKind::Main,
    ))
    .chain(others)
    .collect()
}

/// The mark as the editor shows it, over the whole of its line in `text` (the file as it is
/// now).
pub(super) fn mark_diagnostic(mark: &Mark, change: &LandedChange, text: &str) -> Diagnostic {
    let line = mark.line.saturating_sub(1);
    let length = text
        .lines()
        .nth(line as usize)
        .map_or(0, |words| words.encode_utf16().count() as u32);
    let (code, message) = match mark.kind {
        MarkKind::Main => ("change", main_text(change)),
        MarkKind::Other => ("change", other_text(change, &mark.path)),
        MarkKind::NoChange => ("no-change", no_change_text(change)),
    };
    Diagnostic {
        range: Range {
            start: Position::new(line, 0),
            end: Position::new(line, length),
        },
        severity: Some(DiagnosticSeverity::INFORMATION),
        code: Some(NumberOrString::String(code.into())),
        source: Some("conjure".into()),
        message,
        data: Some(Value::String(change.id.clone())),
        ..Diagnostic::default()
    }
}

// The words of the marks.

/// `fill total · haiku · $0.001 · 4 s · parses · also changed: b.py · not written: .env`.
fn main_text(change: &LandedChange) -> String {
    let Some(main) = change.main_file() else {
        return no_change_text(change);
    };
    let others: Vec<String> = change.other_files(main).map(LandedFile::name).collect();
    let also = if others.is_empty() {
        String::new()
    } else {
        format!(" · also changed: {}", others.join(", "))
    };
    let not_written = if change.not_written.is_empty() {
        String::new()
    } else {
        format!(" · not written: {}", change.not_written.join(", "))
    };
    format!(
        "{} · {} · ${:.3} · {:.0} s · {}{also}{not_written}",
        change.goal_text(),
        change.model,
        change.cost.unwrap_or(0.0),
        change.seconds,
        parses_text(main)
    )
}

/// `conjured with a.py: fill total`, and ` · does not parse (line 3)` when the file does not.
fn other_text(change: &LandedChange, path: &PathBuf) -> String {
    let asked = change
        .asked
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let broken = change
        .files
        .iter()
        .find(|file| &file.editor_path == path)
        .and_then(|file| file.parse_errors.first())
        .map_or_else(String::new, |line| {
            format!(" · does not parse (line {line})")
        });
    format!("conjured with {asked}: {}{broken}", change.goal_text())
}

/// `no change · total needs nothing`.
fn no_change_text(change: &LandedChange) -> String {
    format!("no change · {}", change.goal_text())
}

/// `parses`, `does not parse (line 12)`, or `not parsed` for a file no grammar covers.
fn parses_text(file: &LandedFile) -> String {
    match (file.parsed, file.parse_errors.first()) {
        (false, _) => "not parsed".into(),
        (true, None) => "parses".into(),
        (true, Some(line)) => format!("does not parse (line {line})"),
    }
}
