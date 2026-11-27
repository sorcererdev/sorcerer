//! A finding as a diagnostic: on its line, from source `spellbook`, its code the rule's id.

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};

use crate::domain::finding::Finding;
use crate::domain::rule::Severity;

/// The diagnostic for a finding (the whole line; `Finding::line` counts from 1, the protocol
/// from 0): `hint` is a Hint, `warn` a Warning and `error` an Error. A finding the baseline
/// records is a Hint whose message ends `(existing)`, as `spellbook check` shows it.
pub(super) fn finding_mark(finding: &Finding) -> Diagnostic {
    let line = finding.line.saturating_sub(1);
    let message = if finding.existing {
        format!("{} (existing)", finding.message)
    } else {
        finding.message.clone()
    };
    Diagnostic {
        range: Range::new(Position::new(line, 0), Position::new(line + 1, 0)),
        severity: Some(match finding.severity {
            _ if finding.existing => DiagnosticSeverity::HINT,
            Severity::Hint => DiagnosticSeverity::HINT,
            Severity::Warn => DiagnosticSeverity::WARNING,
            Severity::Error => DiagnosticSeverity::ERROR,
        }),
        code: Some(NumberOrString::String(finding.rule.clone())),
        source: Some("spellbook".into()),
        message,
        ..Diagnostic::default()
    }
}
