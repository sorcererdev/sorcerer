//! `conjure FILE:LINE[-END]`: conjures one change at the place and prints it. Nothing is
//! written to the project; the writer's call is recorded in the ledger, and no change record
//! is kept (records are of editor use). The text the command prints is built last in the file.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use similar::TextDiff;
use sorcerer_core::adapters::platform::paths::canonical;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::unit::Span;

use super::RunArgs;
use crate::adapters::wiring::Wire;
use crate::domain::config::ConjureConfig;
use crate::domain::record::Attempt;
use crate::usecases::conjure_change::{
    ChangeDeps, ChangeRequest, ChangedFile, ConjuredChange, conjure_change,
};

/// Conjures at the place the arguments name and prints the change; the exit code is 0, or 2
/// when no change could be conjured (outside a unit, the day spent, the writer failed).
#[allow(clippy::too_many_arguments)]
pub(super) fn conjure_here(
    args: &RunArgs,
    cwd: &Path,
    project: Project,
    config: &ConjureConfig,
    attempt: Attempt,
    wire: Wire,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let Some(target) = &args.target else {
        bail!("give a place to conjure at: FILE:LINE (see --help)");
    };
    let place = Place::parse(target)?;
    let file = project_relative(&project, cwd, &place.file)?;
    let wired = wire(project, config, attempt)?;
    let fences = Fences::for_project(&wired.project)?;
    let cancel = sorcerer_core::Cancel::default();
    let deps = ChangeDeps {
        project: &wired.project,
        view: &*wired.view,
        context: &*wired.context,
        fences: &fences,
        config,
        writer: &*wired.writer,
        ledger: &*wired.ledger,
        clock: &*wired.clock,
        cancel: &cancel,
        name_case: wired.name_case,
    };
    let request = ChangeRequest {
        file,
        line: place.line,
        selection: place.selection,
        finding: args.finding.clone(),
        attempt,
        situation: None,
    };
    match conjure_change(&request, &deps) {
        Ok(change) => {
            write!(out, "{}", change_text(&change, wired.writer.model()))?;
            Ok(0)
        }
        Err(refused) => {
            writeln!(err, "{}", refused.text())?;
            Ok(2)
        }
    }
}

/// A place as typed: `src/cart.py:12` or `src/cart.py:12-20`.
#[derive(Debug, PartialEq, Eq)]
struct Place {
    file: PathBuf,
    /// The cursor's line: the one given, or the first of a range.
    line: u32,
    selection: Option<Span>,
}

impl Place {
    /// Reads `FILE:LINE` or `FILE:START-END`; the colon that counts is the last one, so a
    /// Windows path with a drive letter works.
    fn parse(text: &str) -> Result<Place> {
        let not_a_place = || anyhow!("`{text}` is not a place: write FILE:LINE or FILE:START-END");
        let (file, lines) = text.rsplit_once(':').ok_or_else(not_a_place)?;
        let (start, end) = match lines.split_once('-') {
            Some((start, end)) => (start, Some(end)),
            None => (lines, None),
        };
        let line: u32 = start.parse().map_err(|_| not_a_place())?;
        let selection = match end {
            None => None,
            Some(end) => Some(Span {
                start: line,
                end: end.parse().map_err(|_| not_a_place())?,
            }),
        };
        if file.is_empty() || line == 0 || selection.is_some_and(|s| s.end < s.start) {
            return Err(not_a_place());
        }
        Ok(Place {
            file: file.into(),
            line,
            selection,
        })
    }
}

/// The file as the project knows it: relative to the root. A file outside the project is an
/// error.
fn project_relative(project: &Project, cwd: &Path, file: &Path) -> Result<PathBuf> {
    let absolute = if file.is_absolute() {
        file.to_path_buf()
    } else {
        cwd.join(file)
    };
    let resolved =
        canonical(&absolute).with_context(|| format!("{} cannot be read", file.display()))?;
    match resolved.strip_prefix(&project.root) {
        Ok(relative) => Ok(relative.to_path_buf()),
        Err(_) => bail!("{} is outside the project", file.display()),
    }
}

// The command's text.

/// The change as the command prints it: its goal, each file's diff with whether it parses, the
/// paths not written, then the model, cost and time. A change with no files is `no change ·
/// <goal>`.
fn change_text(change: &ConjuredChange, model: &str) -> String {
    let goal = change.goal.as_deref().unwrap_or("(no goal given)");
    let head = if change.files.is_empty() {
        format!("no change · {goal}\n")
    } else {
        format!("goal: {goal}\n")
    };
    let files: String = change.files.iter().map(file_text).collect();
    format!(
        "{head}{files}{}{}",
        dropped_text(change),
        footer_text(change, model)
    )
}

fn file_text(file: &ChangedFile) -> String {
    let new = if file.new { " (new file)" } else { "" };
    let before = file.before.as_deref().unwrap_or("");
    let diff = TextDiff::from_lines(before, &file.after)
        .unified_diff()
        .context_radius(3)
        .to_string();
    format!(
        "--- {}{new}\n{diff}{}\n",
        forward_slashes(&file.path),
        parses_text(file)
    )
}

/// `parses`, `does not parse (line 12)`, or `not parsed (no grammar for this file)`.
fn parses_text(file: &ChangedFile) -> String {
    match (file.parsed, file.parse_errors.first()) {
        (false, _) => "not parsed (no grammar for this file)".into(),
        (true, None) => "parses".into(),
        (true, Some(line)) => format!("does not parse (line {line})"),
    }
}

fn dropped_text(change: &ConjuredChange) -> String {
    if change.dropped.is_empty() {
        return String::new();
    }
    let paths: Vec<&str> = change
        .dropped
        .iter()
        .map(|(path, _)| path.as_str())
        .collect();
    format!("not written: {}\n", paths.join(", "))
}

/// `haiku · $0.001 · 4 s`.
fn footer_text(change: &ConjuredChange, model: &str) -> String {
    let cost = change.errand.cost_usd.unwrap_or(0.0);
    format!("{model} · ${cost:.3} · {:.0} s\n", change.errand.seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_a_file_and_a_line_or_a_range() {
        assert_eq!(
            Place::parse("src/a.txt:12").unwrap(),
            Place {
                file: "src/a.txt".into(),
                line: 12,
                selection: None
            }
        );
        assert_eq!(
            Place::parse("C:\\p\\a.txt:3-5").unwrap(),
            Place {
                file: "C:\\p\\a.txt".into(),
                line: 3,
                selection: Some(Span { start: 3, end: 5 })
            }
        );
        for bad in ["a.txt", "a.txt:x", ":3", "a.txt:0", "a.txt:5-3", "a.txt:3-"] {
            assert!(Place::parse(bad).is_err(), "{bad}");
        }
    }
}
