//! After a rules retry: what the second answer dropped of the first, and whether Jev thinks
//! each drop was right. Nothing is held back for it; the drops travel with the change.

use std::path::{Path, PathBuf};

use similar::TextDiff;
use sorcerer_core::domain::paths::forward_slashes;

use super::{ChangeDeps, ChangedFile};
use crate::domain::retry_drops::{
    CheckedDrop, DropVerdict, Dropped, FileNames, RetryAsked, drop_questions, dropped_by_retry,
    verdict_of,
};
use crate::domain::spellbook_findings::RuleFinding;

/// The first answer and the retry, as the drop check reads them.
pub(super) struct TwoAnswers<'a> {
    pub first: &'a [ChangedFile],
    pub retry: &'a [ChangedFile],
    /// The rules the first answer broke.
    pub broken: &'a [RuleFinding],
    pub retry_goal: Option<&'a str>,
}

/// The calls and definitions the first answer had that the retry has nowhere in the change,
/// each with Jev's verdict: legitimate (the rule asked for it, or it moved), looks
/// unintended, or unsure. Without Jev (no key, a failed call) each is not checked. At most
/// 5 are asked; the rest are not checked.
///
/// Example: the first answer wrote `std::fs::write(…)` in a command; the retry deleted the
/// line and added nothing that writes: one drop, which Jev calls unintended.
pub(super) fn drops_of_retry(answers: &TwoAnswers, deps: &ChangeDeps) -> Vec<CheckedDrop> {
    let (first, retry) = names_of_both(answers, deps);
    let dropped = dropped_by_retry(&first, &retry);
    if dropped.is_empty() {
        return Vec::new();
    }
    let verdicts = verdicts_of(&dropped, answers, deps);
    let rules = rule_names(answers.broken);
    dropped
        .into_iter()
        .zip(verdicts)
        .map(|(dropped, verdict)| CheckedDrop {
            dropped,
            verdict,
            rules: rules.clone(),
        })
        .collect()
}

/// Every file either answer touched, as each answer would leave it (a file one answer left
/// alone is its text before the change), with its names. A file no grammar covers is left out
/// of both.
fn names_of_both(answers: &TwoAnswers, deps: &ChangeDeps) -> (Vec<FileNames>, Vec<FileNames>) {
    let paths = touched_paths(answers);
    paths
        .iter()
        .filter_map(|path| {
            let first = text_after(answers.first, answers.retry, path);
            let retry = text_after(answers.retry, answers.first, path);
            Some((
                file_names(path, first, deps)?,
                file_names(path, retry, deps)?,
            ))
        })
        .unzip()
}

fn touched_paths(answers: &TwoAnswers) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for file in answers.first.iter().chain(answers.retry) {
        if !paths.contains(&file.path) {
            paths.push(file.path.clone());
        }
    }
    paths
}

/// The file's text after `answer`, or before the change when `answer` left it alone (empty for
/// a file only the other answer created).
fn text_after(answer: &[ChangedFile], other: &[ChangedFile], path: &Path) -> String {
    if let Some(file) = answer.iter().find(|file| file.path == path) {
        return file.after.clone();
    }
    other
        .iter()
        .find(|file| file.path == path)
        .and_then(|file| file.before.clone())
        .unwrap_or_default()
}

fn file_names(path: &Path, text: String, deps: &ChangeDeps) -> Option<FileNames> {
    let names = deps.context.names_in(path, &text)?;
    Some(FileNames {
        path: forward_slashes(path),
        text,
        names,
    })
}

/// One verdict per drop, in order: Jev's for the first 5, not checked for the rest and for all
/// when there is no Jev or its call failed.
fn verdicts_of(dropped: &[Dropped], answers: &TwoAnswers, deps: &ChangeDeps) -> Vec<DropVerdict> {
    let not_checked = vec![DropVerdict::NotChecked; dropped.len()];
    let Some(decider) = deps.decider else {
        return not_checked;
    };
    let questions = drop_questions(dropped, &retry_asked(answers));
    let Ok(answered) = decider.ask(&questions, deps.cancel) else {
        return not_checked;
    };
    answered
        .iter()
        .map(verdict_of)
        .chain(std::iter::repeat(DropVerdict::NotChecked))
        .take(dropped.len())
        .collect()
}

/// What every drop's question shares: the broken rules, the retry's goal and both answers as
/// diffs.
fn retry_asked(answers: &TwoAnswers) -> RetryAsked {
    let rules: String = answers
        .broken
        .iter()
        .map(|finding| {
            format!(
                "- {} ({}:{}): {}\n",
                finding.rule, finding.file, finding.line, finding.message
            )
        })
        .collect();
    RetryAsked {
        rules,
        goal: answers.retry_goal.unwrap_or("(no goal given)").to_string(),
        first_diff: diff_of(answers.first),
        retry_diff: diff_of(answers.retry),
    }
}

/// The answer's files as one unified diff against their texts before the change.
fn diff_of(files: &[ChangedFile]) -> String {
    files
        .iter()
        .map(|file| {
            let before = file.before.as_deref().unwrap_or("");
            let path = forward_slashes(&file.path);
            TextDiff::from_lines(before, &file.after)
                .unified_diff()
                .context_radius(3)
                .header(&path, &path)
                .to_string()
        })
        .collect()
}

/// `commands-use-ports: Commands talk to ports`, each rule once.
fn rule_names(broken: &[RuleFinding]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for finding in broken {
        let name = format!("{}: {}", finding.rule, finding.message);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}
