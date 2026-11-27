//! The sentences lackey says about tasks: the offer, the done message, the mark on a
//! changed file, a task's line in `lackey log`, and the small formats they share.

use crate::domain::task::{TaskRecord, TaskState};

/// The most characters of the instruction a title keeps.
const TITLE_CHARACTERS: usize = 48;

/// Dollars with two decimals: `$0.84`.
pub fn usd(dollars: f64) -> String {
    format!("${dollars:.2}")
}

/// A duration as the done message says it: `9s`, `1m35s`.
pub fn seconds_text(seconds: f64) -> String {
    let whole = seconds.round().max(0.0) as u64;
    match whole / 60 {
        0 => format!("{whole}s"),
        minutes => format!("{minutes}m{:02}s", whole % 60),
    }
}

/// A task's title: the instruction's first sentence, its first letter lowered, cut at 48
/// characters with `…`.
///
/// Example: `Cancelling less than two hours before the slot should be refused.` is
/// `cancelling less than two hours before the slot…`.
pub fn title_of(instruction: &str) -> String {
    let squashed = instruction.split_whitespace().collect::<Vec<_>>().join(" ");
    let first_sentence = squashed
        .split_inclusive(['.', '?', '!'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(['.', '?', '!'])
        .trim();
    let lowered = lowered_first_letter(first_sentence);
    if lowered.chars().count() <= TITLE_CHARACTERS {
        return lowered;
    }
    let kept: String = lowered.chars().take(TITLE_CHARACTERS - 1).collect();
    format!("{}…", kept.trim_end())
}

fn lowered_first_letter(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The code action that offers an instruction, with its estimate and model:
/// `lackey: do this · ~$0.50 · sonnet`; when the day's budget is spent,
/// `lackey: do this · budget spent`.
pub fn offer_title(estimate: Option<f64>, model: &str) -> String {
    match estimate {
        Some(dollars) => format!("lackey: do this · ~{} · {model}", usd(dollars)),
        None => "lackey: do this · budget spent".to_string(),
    }
}

/// How many files, said: `1 file`, `3 files`.
pub fn files_text(count: usize) -> String {
    match count {
        1 => "1 file".to_string(),
        n => format!("{n} files"),
    }
}

/// The message at the end of a task:
/// `lackey: done · refuse late cancellations · 4 files · $0.71 · 1m35s`. A task that
/// failed or was stopped says so in place of `done`.
pub fn done_text(task: &TaskRecord, seconds: f64) -> String {
    let word = match task.state {
        TaskState::Failed => "failed",
        TaskState::Stopped => "stopped",
        _ => "done",
    };
    let mut parts = vec![
        format!("lackey: {word}"),
        title_of(&task.instruction),
        files_text(task.files.len()),
        usd(task.cost_usd),
        seconds_text(seconds),
    ];
    if let (TaskState::Failed, Some(note)) = (task.state, &task.note) {
        parts.push(note.clone());
    }
    parts.join(" · ")
}

/// The mark on the first changed line of a file a task changed:
/// `✦ lackey: refuse late cancellations · 3 files · $0.84`, or `… · other file` on a file
/// other than the one asked about.
pub fn task_mark_text(task: &TaskRecord, the_asked_file: bool) -> String {
    let which = if the_asked_file {
        files_text(task.files.len())
    } else {
        "other file".to_string()
    };
    format!(
        "\u{2726} lackey: {} · {which} · {}",
        title_of(&task.instruction),
        usd(task.cost_usd)
    )
}

/// One task in `lackey log`: `t3f9a01c2  done  2 files  $0.71  cancel_booking.py:31
/// refuse late cancellations`.
pub fn task_line(task: &TaskRecord) -> String {
    format!(
        "{}  {:<11}  {:<7}  {}  {}:{}  {}",
        task.id,
        state_word(task.state),
        files_text(task.files.len()),
        usd(task.cost_usd),
        task.file,
        task.line,
        title_of(&task.instruction)
    )
}

/// A state as a record names it.
pub fn state_word(state: TaskState) -> &'static str {
    match state {
        TaskState::Running => "running",
        TaskState::Asking => "asking",
        TaskState::Done => "done",
        TaskState::Failed => "failed",
        TaskState::Stopped => "stopped",
        TaskState::Interrupted => "interrupted",
        TaskState::Undone => "undone",
        TaskState::Kept => "kept",
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;
    use crate::domain::task::TaskFile;

    fn task() -> TaskRecord {
        let mut task = TaskRecord::started(
            "t1",
            "Refuse late cancellations. Two hours before.",
            "src/cancel_booking.txt",
            31,
            DateTime::from_timestamp(0, 0).unwrap(),
        );
        task.files = ["a", "b", "c", "d"]
            .iter()
            .map(|path| TaskFile {
                path: path.to_string(),
                new: false,
            })
            .collect();
        task.cost_usd = 0.71;
        task.state = TaskState::Done;
        task
    }

    #[test]
    fn the_done_message_names_the_title_files_cost_and_time() {
        assert_eq!(
            done_text(&task(), 95.0),
            "lackey: done · refuse late cancellations · 4 files · $0.71 · 1m35s"
        );
    }

    #[test]
    fn a_title_is_the_first_sentence_lowered_and_cut() {
        assert_eq!(
            title_of("Refuse late cancellations."),
            "refuse late cancellations"
        );
        let long = title_of("Cancelling less than two hours before the slot should be refused.");
        assert_eq!(long, "cancelling less than two hours before the slot…");
    }

    #[test]
    fn the_offer_names_its_estimate_or_the_spent_budget() {
        assert_eq!(
            offer_title(Some(0.5), "sonnet"),
            "lackey: do this · ~$0.50 · sonnet"
        );
        assert_eq!(
            offer_title(None, "sonnet"),
            "lackey: do this · budget spent"
        );
    }

    #[test]
    fn a_mark_names_the_task_and_whether_it_is_the_asked_file() {
        assert_eq!(
            task_mark_text(&task(), true),
            "✦ lackey: refuse late cancellations · 4 files · $0.71"
        );
        assert!(task_mark_text(&task(), false).contains("· other file ·"));
    }

    #[test]
    fn durations_are_seconds_then_minutes() {
        assert_eq!(seconds_text(9.4), "9s");
        assert_eq!(seconds_text(95.0), "1m35s");
    }
}
