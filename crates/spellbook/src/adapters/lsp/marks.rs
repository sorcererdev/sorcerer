//! A hole as a diagnostic: an Information mark on the placeholder's line, from source
//! `spellbook`, code `hole`, carrying the generation's id. The words are built last in the file.

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use serde_json::Value;

use crate::domain::record::OpenHole;

/// The mark on a hole's line (the whole line; `OpenHole::line` counts from 1, the protocol
/// from 0).
///
/// Example: `handle (block): its first case, from the test file beside it`.
pub(super) fn hole_mark(hole: &OpenHole) -> Diagnostic {
    let line = hole.line.saturating_sub(1);
    Diagnostic {
        range: Range::new(Position::new(line, 0), Position::new(line + 1, 0)),
        severity: Some(DiagnosticSeverity::INFORMATION),
        code: Some(NumberOrString::String("hole".into())),
        source: Some("spellbook".into()),
        message: mark_text(hole),
        data: Some(Value::String(hole.generation.clone())),
        ..Diagnostic::default()
    }
}

fn mark_text(hole: &OpenHole) -> String {
    format!("{} ({}): {}", hole.name, hole.kind, hole.instruction)
}
