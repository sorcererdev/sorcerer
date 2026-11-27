//! A file's marks, published as one list of diagnostics (source `apprentice`): the regions'
//! (an answer, a verdict, an offer), the finished tasks' (`✦` on each changed file), what the
//! agent said with `mark`, and the questions waiting on a line.

use std::path::Path;

use lsp_types::{
    Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location, NumberOrString,
    Position, Range,
};
use serde_json::json;
use sorcerer_lsp::uri_of;

use crate::domain::log_text::{seconds_text, task_mark_text, usd};
use crate::domain::mediums::MarkSeverity;
use crate::domain::region_record::{RegionRecord, ReplyVerdict};
use crate::domain::triage::Kind;

use super::asks::Question;
use super::server_state::{Server, lock};

/// The diagnostics' source.
pub(super) const SOURCE: &str = "apprentice";

impl Server {
    /// Publishes every mark of a file.
    pub(super) fn publish(&self, path: &Path) {
        let text = self.text_now(path);
        let line_range = |start: u32, end: u32| lines_of(&text, start, end);
        let found = self.records_found(path);
        let mut diagnostics: Vec<Diagnostic> = found
            .iter()
            .enumerate()
            .filter(|(index, _)| !inside_another_offer(&found, *index))
            .filter_map(|(_, (record, start, end))| {
                self.region_mark(record, line_range(*start, *end))
            })
            .collect();
        diagnostics.extend(self.task_marks(path, &line_range));
        diagnostics.extend(self.said_marks(path, &line_range));
        diagnostics.extend(self.ask_marks(path, &line_range));
        self.client.publish(path, diagnostics);
    }

    fn region_mark(&self, record: &RegionRecord, range: Range) -> Option<Diagnostic> {
        let kind = record.acting_kind()?;
        let (message, severity) = match (&record.reply, kind) {
            (Some(reply), _) if reply.verdict == ReplyVerdict::Failed => return None,
            (Some(reply), _) => (
                reply_text(
                    record,
                    reply.verdict,
                    &reply.text,
                    reply.cost_usd,
                    reply.seconds,
                ),
                DiagnosticSeverity::INFORMATION,
            ),
            (None, Kind::Instruction) => (
                format!("apprentice: I can do this · {}", self.offer_price()),
                DiagnosticSeverity::HINT,
            ),
            (None, _) => return None,
        };
        let related = record.reply.as_ref().map(|reply| {
            reply
                .citations
                .iter()
                .map(|(file, line)| DiagnosticRelatedInformation {
                    location: Location {
                        uri: uri_of(&self.root.join(file)),
                        range: Range {
                            start: Position::new(line.saturating_sub(1), 0),
                            end: Position::new(line.saturating_sub(1), 0),
                        },
                    },
                    message: format!("{file}:{line}"),
                })
                .collect::<Vec<_>>()
        });
        Some(Diagnostic {
            range,
            severity: Some(severity),
            code: Some(NumberOrString::String("region".into())),
            source: Some(SOURCE.into()),
            message: self.with_citations(message, record),
            related_information: related
                .filter(|related| !related.is_empty() && self.client.related_information()),
            data: Some(json!({ "region": record.region.id })),
            ..Diagnostic::default()
        })
    }

    /// An editor that shows no related information gets the citations in the message.
    fn with_citations(&self, message: String, record: &RegionRecord) -> String {
        let citations = record.reply.as_ref().map(|reply| &reply.citations);
        match citations {
            Some(citations) if !citations.is_empty() && !self.client.related_information() => {
                let cited: Vec<String> = citations
                    .iter()
                    .map(|(file, line)| format!("{file}:{line}"))
                    .collect();
                format!("{message}\n{}", cited.join(", "))
            }
            _ => message,
        }
    }

    /// The offer's price part: `~$0.50 · sonnet`.
    pub(super) fn offer_price(&self) -> String {
        self.offer_title()
            .strip_prefix("apprentice: do this · ")
            .unwrap_or_default()
            .to_string()
    }

    fn task_marks(&self, path: &Path, line_range: &dyn Fn(u32, u32) -> Range) -> Vec<Diagnostic> {
        lock(&self.shown)
            .iter()
            .flat_map(|shown| {
                shown
                    .files
                    .iter()
                    .enumerate()
                    .filter(|(_, file)| !file.settled && file.editor_path == path)
                    .map(move |(n, file)| Diagnostic {
                        range: line_range(file.first_changed_line, file.first_changed_line),
                        severity: Some(DiagnosticSeverity::INFORMATION),
                        code: Some(NumberOrString::String("task".into())),
                        source: Some(SOURCE.into()),
                        message: task_mark_text(&shown.record, n == 0),
                        data: Some(json!({ "task": shown.record.id })),
                        ..Diagnostic::default()
                    })
            })
            .collect()
    }

    fn said_marks(&self, path: &Path, line_range: &dyn Fn(u32, u32) -> Range) -> Vec<Diagnostic> {
        lock(&self.said)
            .get(path)
            .into_iter()
            .flatten()
            .map(|said| Diagnostic {
                range: line_range(said.line, said.line),
                severity: Some(match said.severity {
                    MarkSeverity::Information => DiagnosticSeverity::INFORMATION,
                    MarkSeverity::Warning => DiagnosticSeverity::WARNING,
                    MarkSeverity::Hint => DiagnosticSeverity::HINT,
                }),
                code: Some(NumberOrString::String("said".into())),
                source: Some(SOURCE.into()),
                message: format!("apprentice: {}", said.text),
                data: Some(json!({ "task": said.task })),
                ..Diagnostic::default()
            })
            .collect()
    }

    fn ask_marks(&self, path: &Path, line_range: &dyn Fn(u32, u32) -> Range) -> Vec<Diagnostic> {
        lock(&self.asks)
            .iter()
            .filter(|pending| pending.editor_path == path)
            .map(|pending| Diagnostic {
                range: line_range(pending.line, pending.line),
                severity: Some(DiagnosticSeverity::INFORMATION),
                code: Some(NumberOrString::String("ask".into())),
                source: Some(SOURCE.into()),
                message: Question::of(&pending.question).mark_text(),
                data: Some(json!({ "ask": pending.id })),
                ..Diagnostic::default()
            })
            .collect()
    }
}

/// What a replied region says: `apprentice: holds`, `apprentice: does not hold: <why>`,
/// `apprentice: cannot tell: <why>`, or the answer, then its cost and time.
fn reply_text(
    record: &RegionRecord,
    verdict: ReplyVerdict,
    text: &str,
    cost: f64,
    seconds: f64,
) -> String {
    let said = match verdict {
        ReplyVerdict::Holds => "holds".to_string(),
        ReplyVerdict::DoesNotHold => format!("does not hold: {}", text.trim()),
        ReplyVerdict::CannotTell => format!("cannot tell: {}", text.trim()),
        ReplyVerdict::Answered if record.reply.as_ref().is_some_and(|reply| reply.long) => {
            format!("{} (the whole answer is in a note)", first_sentence(text))
        }
        ReplyVerdict::Answered | ReplyVerdict::Failed => text.trim().to_string(),
    };
    format!(
        "apprentice: {said}\n{} · {}",
        usd(cost),
        seconds_text(seconds)
    )
}

fn first_sentence(text: &str) -> String {
    text.split_inclusive(". ")
        .next()
        .unwrap_or(text)
        .trim()
        .to_string()
}

/// Whole lines `start..=end` (from 1) of the text, ending at the end of the last one (in
/// UTF-16 units, the protocol's default); a line past the end is the last line.
pub(super) fn lines_of(text: &str, start: u32, end: u32) -> Range {
    let lines: Vec<&str> = text.lines().collect();
    let last = lines.len().max(1) as u32;
    let (start, end) = (start.clamp(1, last), end.clamp(1, last));
    let length = lines
        .get(end as usize - 1)
        .map_or(0, |line| line.encode_utf16().count() as u32);
    Range {
        start: Position::new(start - 1, 0),
        end: Position::new(end - 1, length),
    }
}
/// Whether a region's offer sits inside another region's offer (a comment inside a stub): its
/// mark would say "I can do this" twice for one thing, so only the larger one is drawn.
fn inside_another_offer(found: &[(RegionRecord, u32, u32)], index: usize) -> bool {
    let (record, start, end) = &found[index];
    record.offers_a_task()
        && found
            .iter()
            .enumerate()
            .any(|(other, (outer, outer_start, outer_end))| {
                let holds = outer_start <= start && end <= outer_end;
                let larger = outer_end - outer_start > end - start || other < index;
                other != index && outer.offers_a_task() && holds && larger
            })
}
