//! A finding's fixes as quick fixes: what the lightbulb (VS Code) and `vim.lsp.buf.code_action()`
//! (Neovim) list on a line with a spellbook finding.
//!
//! The server only offers and carries the facts. The action's command, `spellbook.fix`, is the
//! editor's own (it is not in `executeCommandProvider`): the editor asks for the inputs still
//! missing, then plans with `spellbook.plan` and applies with `spellbook.apply`.

use lsp_server::ResponseError;
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, Command, Range,
};
use serde_json::{Value, json};
use sorcerer_lsp::path_of;

use super::Server;
use super::commands::invalid;
use super::findings::finding_mark;
use crate::adapters::wiring::Wired;
use crate::domain::finding::Finding;
use crate::usecases::fixes_for::{FixOffer, fixes_for};

/// The client command a fix's action runs.
pub(super) const FIX: &str = "spellbook.fix";

impl Server {
    /// `textDocument/codeAction`: for each finding published on the range's lines, each of its
    /// rule's fixes, the first one preferred. Nothing when the editor asked only for other kinds,
    /// when spellbook cannot run in the project (not trusted, turned off, rules that do not
    /// load), or when no finding there has a fix.
    ///
    /// Example: on `import sqlite3` in bistro's `close_table.py`, two actions, `spellbook: Use a
    /// port the service has` (preferred) and `spellbook: Add a port and use it`.
    pub(super) fn code_actions(&self, params: Value) -> Result<Value, ResponseError> {
        let params: CodeActionParams = serde_json::from_value(params).map_err(invalid)?;
        if !quick_fixes_wanted(&params) {
            return Ok(json!([]));
        }
        let Some(path) = path_of(&params.text_document.uri) else {
            return Ok(json!([]));
        };
        let findings = findings_on_lines(self.published_findings(&path), &params.range);
        if findings.is_empty() {
            return Ok(json!([]));
        }
        let Ok(wired) = self.wired_for_commands() else {
            return Ok(json!([]));
        };
        Ok(json!(fix_actions(&wired, &findings)))
    }
}

/// Whether the editor wants quick fixes: it named no kinds, or one that a quick fix is.
fn quick_fixes_wanted(params: &CodeActionParams) -> bool {
    params.context.only.as_ref().is_none_or(|kinds| {
        kinds.iter().any(|kind| {
            let kind = kind.as_str();
            kind.is_empty() || CodeActionKind::QUICKFIX.as_str().starts_with(kind)
        })
    })
}

/// The findings whose line is within the range (lines from 1, the range's from 0).
fn findings_on_lines(findings: Vec<Finding>, range: &Range) -> Vec<Finding> {
    findings
        .into_iter()
        .filter(|finding| {
            let row = finding.line.saturating_sub(1);
            range.start.line <= row && row <= range.end.line
        })
        .collect()
}

/// Each finding's fixes as actions, in the findings' order; only the very first is preferred, so
/// `ctrl+.` and `<leader>mf` land on one.
fn fix_actions(wired: &Wired, findings: &[Finding]) -> Vec<CodeActionOrCommand> {
    let Ok(checking) = wired.checking() else {
        return Vec::new();
    };
    findings
        .iter()
        .flat_map(|finding| {
            let Some(rule) = checking.rules.iter().find(|rule| rule.id == finding.rule) else {
                return Vec::new();
            };
            fixes_for(
                &wired.view,
                &wired.generators,
                rule,
                &finding.file,
                &finding.node_text,
            )
            .into_iter()
            .map(|offer| (finding.clone(), offer))
            .collect()
        })
        .enumerate()
        .map(|(at, (finding, offer))| {
            CodeActionOrCommand::CodeAction(fix_action(&finding, offer, at == 0))
        })
        .collect()
}

/// One fix as a quick fix whose command carries what the editor needs to plan it.
fn fix_action(finding: &Finding, offer: FixOffer, preferred: bool) -> CodeAction {
    CodeAction {
        title: format!("spellbook: {}", offer.label),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![finding_mark(finding)]),
        is_preferred: preferred.then_some(true),
        command: Some(Command {
            title: offer.label.clone(),
            command: FIX.into(),
            arguments: Some(vec![fix_argument(finding, offer)]),
        }),
        ..CodeAction::default()
    }
}

/// The argument of `spellbook.fix`: the generator, the inputs the finding settles, the inputs
/// still to ask, and the finding the fix answers. `version` is left for the editor to fill with
/// the document's version when it runs the command (this server does not follow the text).
///
/// Example: `{ generator: "use-port", inputs: { app: "bistro", … }, missing: ["port"], fix: {
/// file: "src/bistro/booking/commands/close_table.txt", line: 4, rule: "commands-use-ports",
/// index: 1, label: "Use a port the service has", version: null } }`.
fn fix_argument(finding: &Finding, offer: FixOffer) -> Value {
    json!({
        "generator": offer.generator,
        "inputs": offer.inputs,
        "missing": offer.missing,
        "fix": {
            "file": finding.file,
            "line": finding.line,
            "rule": finding.rule,
            "index": offer.index,
            "label": offer.label,
            "version": Value::Null,
        },
    })
}

#[cfg(test)]
mod tests {
    use lsp_types::Position;

    use super::*;
    use crate::domain::rule::Severity;

    fn finding(line: u32) -> Finding {
        Finding {
            region: None,
            rule: "r".into(),
            file: "a.txt".into(),
            line,
            severity: Severity::Warn,
            message: "m".into(),
            fingerprint: "f".into(),
            node_text: "x".into(),
        }
    }

    #[test]
    fn a_finding_is_on_the_range_when_its_line_is() {
        let range = Range::new(Position::new(3, 0), Position::new(4, 0));
        let lines: Vec<u32> =
            findings_on_lines(vec![finding(3), finding(4), finding(5), finding(6)], &range)
                .iter()
                .map(|finding| finding.line)
                .collect();
        assert_eq!(lines, [4, 5]);
    }

    #[test]
    fn the_editor_may_ask_for_quick_fixes_by_a_wider_kind() {
        let wanting = |only: Option<Vec<&str>>| {
            let params: CodeActionParams = serde_json::from_value(json!({
                "textDocument": { "uri": "file:///a.txt" },
                "range": { "start": { "line": 0, "character": 0 },
                           "end": { "line": 0, "character": 0 } },
                "context": { "diagnostics": [], "only": only },
            }))
            .unwrap();
            quick_fixes_wanted(&params)
        };
        assert!(wanting(None));
        assert!(wanting(Some(vec!["quickfix"])));
        assert!(!wanting(Some(vec!["refactor"])));
        assert!(!wanting(Some(vec!["quickfix.other"])));
    }
}
