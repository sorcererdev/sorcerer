//! What a task's agent is told: the rules of working in the person's editor (appended to its
//! system prompt) and the task itself (its first message).

use crate::domain::scry_notes::ScryNote;
use sorcerer_core::domain::paths::forward_slashes;

/// The rules of the editor, appended to the agent's system prompt.
pub const EDITOR_RULES: &str = "You are working inside the person's code editor, not in a chat. Nobody reads your text output: there is no transcript. Everything you want the person to see goes through the apprentice tools: notify (one sentence), mark (a line in the margin of a file, with hover text), ghost (grey suggested lines), note (more than a paragraph, opened beside the code), ask (a decision, with options), and write_file, the only way to change a file. Edit, Write and the other file tools are not available. The person keeps working while you do: after each tool call you may be told what they changed, and their edits are authoritative; keep them and build on them. When write_file is refused because a file moved on, rebase on its text now and write again. Do the one task you are given, verify it (run the project's tests when there are any), then stop with one notify that says what you did.";

/// Where the task was asked and what surrounds it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskPlace {
    /// The instruction in the person's words (the region's text).
    pub instruction: String,
    /// The file (relative to the root) and the line, from 1.
    pub file: String,
    pub line: u32,
    /// The unit holding the line, when there is one: its name and its text.
    pub unit: Option<(String, String)>,
    /// scry's open findings near the place.
    pub notes: Vec<ScryNote>,
    /// The project's rules (scry's steering file), possibly empty.
    pub rules: String,
    /// The open files' unsaved edits, as `unsaved_edits_text` says them.
    pub unsaved: Option<String>,
}

/// The task's first message.
///
/// Example: for the instruction `Cancelling less than two hours before should be refused.` at
/// `src/cancel_booking.py:31` in `CancelBooking.handle`, the message opens with the
/// instruction, says where it was written and shows the unit.
pub fn task_prompt(place: &TaskPlace) -> String {
    let sections = [
        Some(format!(
            "The person wrote this in their code, and asked you to do it:\n\n{}",
            place.instruction.trim()
        )),
        Some(format!(
            "They wrote it in {} at line {}.",
            place.file, place.line
        )),
        place
            .unit
            .as_ref()
            .map(|(name, text)| format!("It sits in {name}:\n\n```\n{}\n```", text.trim_end())),
        notes_text(&place.notes),
        (!place.rules.trim().is_empty())
            .then(|| format!("The project's rules:\n\n{}", place.rules.trim())),
        place.unsaved.clone(),
    ];
    sections
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn notes_text(notes: &[ScryNote]) -> Option<String> {
    if notes.is_empty() {
        return None;
    }
    let lines: Vec<String> = notes
        .iter()
        .map(|note| {
            let stale = if note.stale {
                " (may be out of date)"
            } else {
                ""
            };
            format!(
                "- {}:{} {}: {}{stale}",
                forward_slashes(&note.file),
                note.line,
                note.issue_type,
                note.statement
            )
        })
        .collect();
    Some(format!(
        "The reviewer's open findings near it, to act on or leave:\n{}",
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_says_what_where_and_around() {
        let place = TaskPlace {
            instruction: "Refuse late cancellations.".into(),
            file: "src/cancel.txt".into(),
            line: 31,
            unit: Some((
                "CancelBooking.handle".into(),
                "def handle(self):\n    pass\n".into(),
            )),
            notes: Vec::new(),
            rules: "- Money is cents.".into(),
            unsaved: None,
        };
        let text = task_prompt(&place);
        assert!(
            text.starts_with("The person wrote this in their code"),
            "{text}"
        );
        assert!(text.contains("src/cancel.txt at line 31"), "{text}");
        assert!(text.contains("It sits in CancelBooking.handle:"), "{text}");
        assert!(
            text.contains("The project's rules:\n\n- Money is cents."),
            "{text}"
        );
        assert!(!text.contains("findings"), "{text}");
    }

    #[test]
    fn the_rules_hold_no_newline_free_promise_but_name_every_medium() {
        for medium in ["notify", "mark", "ghost", "note", "ask", "write_file"] {
            assert!(EDITOR_RULES.contains(medium), "{medium}");
        }
        assert!(!EDITOR_RULES.contains('\n'));
    }
}
