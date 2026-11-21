//! `scry [FILES]...`: reviews the change the arguments name and prints the
//! open findings in it, the most confident first. Every run writes a run
//! log, whatever happened, so a run can be looked at afterwards with
//! `scry log`.
//!
//! [`logged`] keeps the log around the run; [`run`] is the review. The
//! text the command prints and logs is built last in the file.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Local, Utc};
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::ledger::Ledger;

use super::format::{Output, finding_json, finding_text, relative_or_absolute};
use super::{Console, RunArgs};
use crate::adapters::markdown_run_log::{self, RESULT, StreamingLog};
use crate::adapters::scry_toml::{STEERING_FILE, ScryToml};
use crate::adapters::wiring::Wiring;
use crate::domain::config::{ScryConfig, scry_config};
use crate::domain::finding::{Finding, Status, status_word};
use crate::domain::findings::Findings;
use crate::domain::log_text::{block, bytes, count, section, usd};
use crate::domain::scope::ScopeRequest;
use crate::domain::steering::{STEERING_CAP, Steering};
use crate::ports::run_log::RunLog;
use crate::ports::settings::Settings;
use crate::usecases::read_findings;
use crate::usecases::review_change::{
    ReviewDeps, ReviewOutcome, ReviewReport, ReviewRequest, review_change,
};

/// Runs a review with its run log, and gives back the exit code:
///
/// 1. the log starts, streaming to stderr as it grows under `-v`;
/// 2. its head says when and where scry ran, the command, the settings and
///    the steering it read;
/// 3. the review runs ([`run`]);
/// 4. if the review could not run, the log ends with why;
/// 5. the log is written under the project's `state/scry/runs/`, whatever
///    happened. If writing it fails, that is one line on stderr.
pub(super) fn logged<W: Wiring>(
    args: &RunArgs,
    line: &str,
    cwd: &Path,
    project: &Project,
    wire: impl Fn(&Project) -> Result<W>,
    console: &mut Console,
) -> Result<i32> {
    let log = StreamingLog::new(&mut *console.err, args.verbose);
    let wired = wire(project);
    let (now, offset) = clock_of(&wired);
    log.put(&head(args, line, project, now, offset));
    let result = match wired {
        Ok(wiring) => run(
            args,
            cwd,
            project,
            &wiring,
            &mut *console.out,
            &log,
            console.width,
        ),
        Err(e) => Err(e),
    };
    if let Err(e) = &result {
        log.put(&error_summary_text(e));
    }
    if let Err(e) = write_run_log(project, now, &log) {
        log.say(&run_log_not_written_text(&e))?;
    }
    result
}

/// The time now and the user's time zone, from the wiring's clock (the
/// tests' fixed clock), or the machine's when the wiring failed.
fn clock_of<W: Wiring>(wired: &Result<W>) -> (DateTime<Utc>, FixedOffset) {
    match wired {
        Ok(wiring) => {
            let deps = wiring.deps();
            (deps.clock.now(), deps.clock.offset())
        }
        Err(_) => (Utc::now(), *Local::now().offset()),
    }
}

/// Writes the log's text to a file under the project's `state/scry/runs/`,
/// named for the time the run started.
fn write_run_log(project: &Project, now: DateTime<Utc>, log: &StreamingLog) -> Result<()> {
    let state = project.state_dir()?;
    markdown_run_log::write_run(&markdown_run_log::runs_dir(&state), now, &log.text())?;
    Ok(())
}

/// The log's head: when and where scry ran, which version, the command,
/// the caps and the `[scry]` settings in force, then the steering read.
fn head(
    args: &RunArgs,
    line: &str,
    project: &Project,
    now: DateTime<Utc>,
    offset: FixedOffset,
) -> String {
    let config = match scry_config(project) {
        Ok(cfg) => config_text(&cfg, args.budget),
        Err(e) => format!("- config: {e}\n"),
    };
    let steering = steering_section(&ScryToml::for_project(project), &project.root);
    format!("{}{config}{steering}", run_text(line, project, now, offset))
}

/// The steering the run read: the file, the size of its context, each
/// included file's size, the rule ids, and the size of the whole against
/// the 32 KB cap. A steering file that does not read is its error.
///
/// Example:
/// ```text
/// ## Steering
///
/// - file: .sorcerer/scry.toml
/// - context: 312 B
/// - include docs/conventions.md: 1.2 KB
/// - rules: log-text-apart, plain-names
/// - size: 2.0 KB of 32.0 KB
/// ```
fn steering_section(settings: &dyn Settings, root: &Path) -> String {
    let lines = match settings.steering() {
        Err(e) => vec![format!("- error: {e}")],
        Ok(steering) => steering_lines(&steering, settings.steering_source(), root),
    };
    let body: String = lines.iter().map(|line| format!("{line}\n")).collect();
    section("Steering") + &body
}

fn steering_lines(steering: &Steering, source: Option<PathBuf>, root: &Path) -> Vec<String> {
    let file = match source {
        Some(file) => format!("- file: {}", relative_or_absolute(&file, root)),
        None => format!("- none: no {STEERING_FILE}"),
    };
    let context = steering.context.trim();
    let context_size = if context.is_empty() {
        "none".to_string()
    } else {
        bytes(context.len())
    };
    let includes = steering.includes.iter().map(|include| {
        let size = bytes(include.text.len());
        format!("- include {}: {size}", forward_slashes(&include.path))
    });
    let ids: Vec<&str> = steering.rules.iter().map(|r| r.id.as_str()).collect();
    let rules = if ids.is_empty() {
        "none".to_string()
    } else {
        ids.join(", ")
    };
    let size = format!(
        "- size: {} of {}",
        bytes(steering.size()),
        bytes(STEERING_CAP)
    );
    [file, format!("- context: {context_size}")]
        .into_iter()
        .chain(includes)
        .chain([format!("- rules: {rules}"), size])
        .collect()
}

/// The review itself, and its exit code:
///
/// 1. a `--budget` must be a number of dollars, 0 or more;
/// 2. the pass reviews the change the arguments name. If it was skipped
///    (no change, say), the reason is said and the exit code is 2;
/// 3. the open findings in the change are printed, the most confident first;
/// 4. the summary goes into the log, and its last line is said;
/// 5. what failed or what a cap held back is said on stderr.
fn run(
    args: &RunArgs,
    cwd: &Path,
    project: &Project,
    wired: &impl Wiring,
    out: &mut dyn Write,
    log: &StreamingLog,
    width: usize,
) -> Result<i32> {
    check_budget(args.budget)?;
    let cfg = scry_config(project)?;
    let deps = ReviewDeps {
        log,
        ..wired.deps()
    };
    let rows_before = ledger_rows(deps.ledger).len();
    let report = match review_change(project, &request_from(args, cwd), &deps)? {
        ReviewOutcome::Skipped(why) => {
            log.put(&skipped_summary_text(&why));
            log.say(&format!("scry: {why}"))?;
            return Ok(2);
        }
        ReviewOutcome::Ran(report) => report,
    };
    let stored = read_findings(deps.store)?;
    let shown = stored.shown_in_scope(&report.scope, &cfg.issue_types);
    let output = Output::json_if(args.json);
    print_findings(out, &shown, output, width)?;
    let code = exit_code(shown.len(), &report);
    let new_rows: Vec<Errand> = ledger_rows(deps.ledger)
        .into_iter()
        .skip(rows_before)
        .collect();
    log.put(&summary_text(
        &report, &stored, &shown, &cfg, &new_rows, code,
    ));
    if !shown.is_empty() || report.looked_at > 0 || report.spent > 0.0 {
        say_summary_line(out, log, output, &summary_line(shown.len(), &report))?;
    }
    let held_back = held_back_text(&report, args.budget);
    if !held_back.is_empty() {
        log.write_err(held_back.as_bytes())?;
    }
    Ok(code)
}

/// A `--budget` is a number of dollars, 0 or more.
fn check_budget(budget: Option<f64>) -> Result<()> {
    if let Some(dollars) = budget
        && (dollars.is_nan() || dollars < 0.0)
    {
        anyhow::bail!("--budget {dollars}: give a number of dollars, 0 or more");
    }
    Ok(())
}

/// The review the arguments ask for: the files changed `--since` a
/// revision, else the files named (relative to `cwd`), else the working
/// tree.
fn request_from(args: &RunArgs, cwd: &Path) -> ReviewRequest {
    let scope = match (&args.since, args.files.is_empty()) {
        (Some(rev), _) => ScopeRequest::Since(rev.clone()),
        (None, false) => ScopeRequest::Files(args.files.iter().map(|f| cwd.join(f)).collect()),
        (None, true) => ScopeRequest::Working,
    };
    ReviewRequest {
        scope,
        all: args.all,
        fresh: args.fresh,
        budget: args.budget,
        caps: None,
    }
}

/// The ledger's rows; none when it cannot be read.
fn ledger_rows(ledger: &dyn Ledger) -> Vec<Errand> {
    ledger.read().map(|r| r.errands).unwrap_or_default()
}

/// Prints the findings: as JSON, one object a line, or as text wrapped to
/// the terminal's width.
fn print_findings(
    out: &mut dyn Write,
    shown: &[&Finding],
    output: Output,
    width: usize,
) -> Result<()> {
    for f in shown {
        match output {
            Output::Json => writeln!(out, "{}", finding_json(f))?,
            Output::Text => write!(out, "{}", finding_text(f, width))?,
        }
    }
    Ok(())
}

/// The exit code: 1 when a finding is shown; else 2 when a call failed or
/// a cap held something back, so the review is not whole; else 0.
fn exit_code(shown: usize, report: &ReviewReport) -> i32 {
    if shown > 0 {
        1
    } else if !report.failures.is_empty() || report.capped > 0 || report.checks_capped {
        2
    } else {
        0
    }
}

/// The summary line on stdout, or on stderr under `--json`, so stdout
/// holds only JSON objects.
fn say_summary_line(
    out: &mut dyn Write,
    log: &StreamingLog,
    output: Output,
    line: &str,
) -> Result<()> {
    match output {
        Output::Json => log.say(line)?,
        Output::Text => writeln!(out, "{line}")?,
    }
    Ok(())
}

// The command's text and the run log's.

/// The line said at the end of a run: `1 finding, 1 unit looked at, $0.04`.
fn summary_line(shown: usize, report: &ReviewReport) -> String {
    format!(
        "{}, {} looked at, ${:.2}",
        count(shown, "finding"),
        count(report.looked_at, "unit"),
        report.spent
    )
}

/// What the user is told on stderr after a run that was not whole: each
/// failed call, the units a cap stopped, and a claim check a cap held back.
/// The cap is named as the budget or the day's cap when `--budget` was
/// given, else as the day's cap.
///
/// Example: `scry: the day's cap stopped 2 units; they wait for the next run`.
fn held_back_text(report: &ReviewReport, budget: Option<f64>) -> String {
    let cap = match budget {
        Some(_) => "the budget or the day's cap",
        None => "the day's cap",
    };
    let failures = report.failures.iter().map(|f| format!("scry: {f}\n"));
    let units_stopped = (report.capped > 0).then(|| {
        let they = if report.capped == 1 {
            "it waits"
        } else {
            "they wait"
        };
        let units = count(report.capped, "unit");
        format!("scry: {cap} stopped {units}; {they} for the next run\n")
    });
    let check_held_back = report.checks_capped.then(|| {
        format!(
            "scry: {cap} held back a claim check; its finding stays unverified until the next run\n"
        )
    });
    failures
        .chain(units_stopped)
        .chain(check_held_back)
        .collect()
}

fn run_log_not_written_text(e: &anyhow::Error) -> String {
    format!("scry: the run log was not written: {e:#}")
}

/// The head's Run section: when (in UTC and the user's time zone), the
/// project, which scry, and the command as typed.
fn run_text(line: &str, project: &Project, now: DateTime<Utc>, offset: FixedOffset) -> String {
    format!(
        "# scry run {}\n{}- time: {} (UTC {})\n- project: `{}`\n- scry {}\n- command: `{line}`\n",
        now.format("%Y-%m-%dT%H:%M:%SZ"),
        section("Run"),
        now.with_timezone(&offset).format("%Y-%m-%d %H:%M:%S %:z"),
        now.format("%Y-%m-%d %H:%M:%S"),
        project.root.display(),
        env!("CARGO_PKG_VERSION"),
    )
}

/// The caps, and the `[scry]` settings in force with the defaults filled
/// in, one `key = value` a line.
fn config_text(cfg: &ScryConfig, budget: Option<f64>) -> String {
    let fields = serde_json::to_value(cfg)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    let text: String = fields.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
    format!(
        "- caps: pass none on the command line, day {}, budget {}\n- config ([scry] with the defaults filled in):\n\n{}",
        usd(cfg.budget_per_day),
        budget.map_or("none".into(), usd),
        block(&text)
    )
}

fn error_summary_text(e: &anyhow::Error) -> String {
    format!(
        "{}- error: {e:#}\n\n{RESULT}could not run, exit 2\n",
        section("Summary")
    )
}

fn skipped_summary_text(why: &str) -> String {
    format!(
        "{}- skipped: {why}\n\n{RESULT}skipped ({why}), exit 2\n",
        section("Summary")
    )
}

/// The log's summary: each finding the pass added or changed, with its
/// status and why it is or is not printed; what was spent, by kind of
/// call; the failures and caps; then the result line.
fn summary_text(
    r: &ReviewReport,
    stored: &Findings,
    shown: &[&Finding],
    cfg: &ScryConfig,
    new_rows: &[Errand],
    code: i32,
) -> String {
    format!(
        "{}{}{}{}\n{RESULT}{} shown, {} looked at, ${:.2}, exit {code}\n",
        section("Summary"),
        touched_text(r, stored, shown, cfg),
        spent_text(r, new_rows),
        failures_and_caps_text(r),
        count(shown.len(), "finding"),
        count(r.looked_at, "unit"),
        r.spent
    )
}

/// A line for each finding the pass added or changed: its status, its
/// verdict, and whether it is printed and if not, why.
///
/// Example: `- 3fa9c0 bug at orders.py:41: withheld; not printed: judged a
/// nitpick (confidence 0.40 under the threshold 0.70)`.
fn touched_text(
    report: &ReviewReport,
    stored: &Findings,
    shown: &[&Finding],
    cfg: &ScryConfig,
) -> String {
    if report.touched.is_empty() {
        return "- no finding added or changed\n".into();
    }
    report
        .touched
        .iter()
        .map(|id| match stored.findings.iter().find(|f| f.id == *id) {
            Some(f) => touched_line(f, report, shown, cfg),
            None => format!("- {}: not in the store as written\n", id.0),
        })
        .collect()
}

fn touched_line(
    f: &Finding,
    report: &ReviewReport,
    shown: &[&Finding],
    cfg: &ScryConfig,
) -> String {
    let in_scope = report.scope.units.iter().any(|scoped| {
        scoped.file == f.place.file && f.place.unit.as_deref() == Some(scoped.unit.name.as_str())
    });
    let verdict = f
        .verdict
        .as_ref()
        .map(|v| format!(" ({}: {})", v.verdict, v.why))
        .unwrap_or_default();
    let printed = if shown.iter().any(|g| g.id == f.id) {
        "printed".to_string()
    } else {
        format!("not printed: {}", why_not_printed(f, in_scope, cfg))
    };
    format!(
        "- {} {} at {}:{}: {}{verdict}; {printed}\n",
        f.id.0,
        f.issue_type,
        forward_slashes(&f.place.file),
        f.place.lines.start,
        status_word(f.status),
    )
}

/// Why a finding the pass added or changed is not printed.
fn why_not_printed(f: &Finding, in_scope: bool, cfg: &ScryConfig) -> String {
    match f.status {
        Status::Open if !cfg.issue_types.contains(&f.issue_type) => format!(
            "its issue type {} is not in [scry] issue_types",
            f.issue_type
        ),
        Status::Open if !in_scope => "its unit is not in this run's scope".into(),
        Status::Open => "open".into(),
        Status::Unverified => "not checked yet (a cap or a failed check)".into(),
        Status::Withheld => format!(
            "judged a nitpick (confidence {:.2} under the threshold {:.2})",
            f.scores.confidence.unwrap_or(0.0),
            cfg.confidence_threshold
        ),
        Status::Refuted => "the claim check said it is wrong".into(),
        Status::Unsure => "the check could not confirm it".into(),
        Status::Elsewhere => "its lines are in no unit in scope".into(),
        Status::DismissedHere | Status::DismissedEverywhere => "dismissed".into(),
        Status::Resolved => "resolved".into(),
        Status::Gone => "its unit is gone".into(),
    }
}

/// What the pass charged, and the ledger's new scry rows counted and
/// summed by kind of call, so the two can be compared.
///
/// Example: `- spent: the pass charged $0.056; the ledger's new scry rows:
/// check 1 for $0.016, review 1 for $0.040`.
fn spent_text(report: &ReviewReport, new_rows: &[Errand]) -> String {
    let by_verb = new_rows
        .iter()
        .filter(|errand| errand.product == "scry")
        .fold(
            BTreeMap::<&str, (usize, f64)>::new(),
            |mut by_verb, errand| {
                let (calls, dollars) = by_verb.entry(errand.verb.as_str()).or_default();
                *calls += 1;
                *dollars += errand.cost_usd.unwrap_or(0.0);
                by_verb
            },
        );
    let verbs: Vec<String> = by_verb
        .iter()
        .map(|(verb, (calls, dollars))| format!("{verb} {calls} for {}", usd(*dollars)))
        .collect();
    format!(
        "- spent: the pass charged {}; the ledger's new scry rows: {}\n",
        usd(report.spent),
        if verbs.is_empty() {
            "none".into()
        } else {
            verbs.join(", ")
        }
    )
}

/// The failed calls (or none), the units a cap stopped, and a claim check
/// a cap held back.
fn failures_and_caps_text(report: &ReviewReport) -> String {
    let no_failures = report
        .failures
        .is_empty()
        .then(|| "- failures: none\n".to_string());
    let failures = report.failures.iter().map(|f| format!("- failure: {f}\n"));
    let capped =
        (report.capped > 0).then(|| format!("- capped: {}\n", count(report.capped, "unit")));
    let check_held_back = report
        .checks_capped
        .then(|| "- a claim check was held back by the caps\n".to_string());
    no_failures
        .into_iter()
        .chain(failures)
        .chain(capped)
        .chain(check_held_back)
        .collect()
}
