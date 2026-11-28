//! A hole as a diagnostic: an Information mark on the placeholder's line, from source
//! `spellbook`, code `hole`, carrying the generation's id. The words are built last in the file.

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use serde_json::Value;

use crate::domain::open_hole_places::placeholder_span;
use crate::domain::record::OpenHole;

/// The mark on a hole's placeholder: the span of the placeholder itself when the row holds it,
/// else the whole line (`OpenHole::line` counts from 1, the protocol from 0; columns are UTF-16).
///
/// Example: `handle (block): its first case, from the test file beside it`, on
/// `raise NotImplementedError("hole handle: …")` and not on the indent before it.
pub(super) fn hole_mark(hole: &OpenHole, row: Option<&str>) -> Diagnostic {
    Diagnostic {
        range: hole_range(hole, row),
        severity: Some(DiagnosticSeverity::INFORMATION),
        code: Some(NumberOrString::String("hole".into())),
        source: Some("spellbook".into()),
        message: mark_text(hole),
        data: Some(Value::String(hole.generation.clone())),
        ..Diagnostic::default()
    }
}

fn hole_range(hole: &OpenHole, row: Option<&str>) -> Range {
    let line = hole.line.saturating_sub(1);
    let Some((row, span)) = row.and_then(|row| Some((row, placeholder_span(hole, row)?))) else {
        return Range::new(Position::new(line, 0), Position::new(line + 1, 0));
    };
    let column = |byte: usize| row[..byte].encode_utf16().count() as u32;
    Range::new(
        Position::new(line, column(span.start)),
        Position::new(line, column(span.end)),
    )
}

fn mark_text(hole: &OpenHole) -> String {
    format!("{} ({}): {}", hole.name, hole.kind, hole.instruction)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hole(placeholder: &str) -> OpenHole {
        OpenHole {
            generation: "g".into(),
            generator: "command".into(),
            file: "a.x".into(),
            line: 3,
            name: "field".into(),
            kind: "ident".into(),
            instruction: "x".into(),
            placeholder: placeholder.into(),
            edges: String::new(),
            frame: Vec::new(),
        }
    }

    #[test]
    fn the_columns_count_utf16_not_bytes() {
        let mark = hole_mark(&hole("todo_field"), Some("é    todo_field: object"));
        assert_eq!(mark.range.start, Position::new(2, 5));
        assert_eq!(mark.range.end, Position::new(2, 15));
    }

    #[test]
    fn a_row_without_the_placeholder_marks_the_whole_line() {
        let mark = hole_mark(&hole("todo_field"), Some("id: str"));
        assert_eq!(
            mark.range,
            Range::new(Position::new(2, 0), Position::new(3, 0))
        );
    }
}
