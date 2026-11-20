//! A finding as the editor shows it: one diagnostic on its line.

use std::path::Path;

use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range,
};
use sorcerer_lsp::uri_of;

use crate::domain::finding::Finding;
use crate::usecases::show_findings::Shown;

/// The line a stale finding's message ends with.
pub const STALE_LINE: &str = "(the code changed since this review)";

/// What the editor shows for one finding, placed in the file whose text is `text`:
///
/// - on its placed line, from its first character to its last;
/// - an error for a `bug`, a warning for every other issue type;
/// - source `scry`, code the issue type (`bug-stale` when its unit changed since);
/// - the message is the statement, then `rule: <id>` when it cites a rule, then
///   `(the code changed since this review)` when stale, one per line;
/// - each piece of evidence as a related location (its file, whole lines), only when the
///   editor shows them (`related`);
/// - the finding's id as its data, so the editor can hand it back.
///
/// Example: an open bug "cancel() returns None." on line 3 is an error on the zero-based
/// line 2, code `bug`, message `cancel() returns None.`.
pub fn diagnostic(s: &Shown, root: &Path, text: &str, related: bool) -> Diagnostic {
    let f = &s.finding;
    let line = s.placed.line.saturating_sub(1);
    Diagnostic {
        range: whole_line(text, line),
        severity: Some(severity_of(&f.issue_type)),
        code: Some(NumberOrString::String(code_of(f, s.placed.stale))),
        source: Some("scry".into()),
        message: message_of(f, s.placed.stale),
        related_information: related.then(|| evidence_of(f, root)),
        data: Some(serde_json::Value::String(f.id.0.clone())),
        ..Diagnostic::default()
    }
}

/// A zero-based line from its start to its end, counted in UTF-16 code units as the protocol
/// counts characters.
fn whole_line(text: &str, line: u32) -> Range {
    let length = text
        .lines()
        .nth(line as usize)
        .map_or(0, |l| l.encode_utf16().count());
    Range {
        start: Position { line, character: 0 },
        end: Position {
            line,
            character: u32::try_from(length).unwrap_or(u32::MAX),
        },
    }
}

fn severity_of(issue_type: &str) -> DiagnosticSeverity {
    if issue_type == "bug" {
        DiagnosticSeverity::ERROR
    } else {
        DiagnosticSeverity::WARNING
    }
}

fn code_of(f: &Finding, stale: bool) -> String {
    if stale {
        format!("{}-stale", f.issue_type)
    } else {
        f.issue_type.clone()
    }
}

fn message_of(f: &Finding, stale: bool) -> String {
    let rule = f.rule.as_ref().map(|id| format!("rule: {id}"));
    let stale = stale.then(|| STALE_LINE.to_string());
    std::iter::once(f.statement.clone())
        .chain(rule)
        .chain(stale)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each cited file's whole lines, as related locations.
fn evidence_of(f: &Finding, root: &Path) -> Vec<DiagnosticRelatedInformation> {
    f.evidence
        .iter()
        .map(|e| DiagnosticRelatedInformation {
            location: Location {
                uri: uri_of(&root.join(&e.file)),
                range: Range {
                    start: Position {
                        line: e.lines.start.saturating_sub(1),
                        character: 0,
                    },
                    end: Position {
                        line: e.lines.end,
                        character: 0,
                    },
                },
            },
            message: "evidence".into(),
        })
        .collect()
}
