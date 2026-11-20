//! `scry [FILES]...`: one review over the scope the arguments name, then
//! every open finding of a shown issue type placed in a scope unit, best
//! first; the run log written whatever happened.
//!
//! `logged` wraps the run: the log's head (`head`), the run (`run`), an
//! error's summary, the log written (`write_run_log`). `run` is the review
//! itself: the request (`request_from`), the pass, the findings printed
//! (`print_findings`), the exit code (`exit_code`), the summary logged and
//! said, and what was held back.
//!
//! The run log's text is built last in the file.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Local, Utc};
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::ledger::Ledger;

use super::RunArgs;
use super::format::{finding_json, finding_text};
use crate::adapters::markdown_run_log::{self, RESULT, StreamingLog};
use crate::adapters::scry_toml::{STEERING_FILE, ScryToml};
use crate::adapters::wiring::Wiring;
use crate::domain::config::{ScryConfig, scry_config};
use crate::domain::finding::{Finding, Status, status_word};
use crate::domain::findings::Findings;
use crate::domain::log_text::{block, bytes, count, section, usd};
use crate::domain::scope::ScopeRequest;
use crate::domain::steering::STEERING_CAP;
use crate::ports::run_log::RunLog;
use crate::ports::settings::Settings;
use crate::usecases::read_findings;
use crate::usecases::review_change::{
    ReviewDeps, ReviewOutcome, ReviewReport, ReviewRequest, review_change,
};

/// A run, logged: the head, the run itself, then the summary's last line;
/// the log is written under `state/scry/runs/` whatever the run did, and a
/// failure to write it is one line on stderr.
#[allow(clippy::too_many_arguments)]
pub(super) fn logged<W: Wiring>(
    a: &RunArgs,
    line: &str,
    cwd: &Path,
    project: &Project,
    wire: impl Fn(&Project) -> Result<W>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    width: usize,
) -> Result<i32> {
    let log = StreamingLog::new(err, a.verbose);
    let wired = wire(project);
    let (now, offset) = clock_of(&wired);
    log.put(&head(a, line, project, now, offset));
    let result = match wired {
        Ok(w) => run(a, cwd, project, &w, out, &log, width),
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

/// The wiring's clock, or the machine's when the wiring failed.
fn clock_of<W: Wiring>(wired: &Result<W>) -> (DateTime<Utc>, FixedOffset) {
    match wired {
        Ok(w) => {
            let d = w.deps();
            (d.clock.now(), d.clock.offset())
        }
        Err(_) => (Utc::now(), *Local::now().offset()),
    }
}

/// LOG's text, under the project's `state/scry/runs/`, named for NOW.
fn write_run_log(project: &Project, now: DateTime<Utc>, log: &StreamingLog) -> Result<()> {
    let state = project.state_dir()?;
    markdown_run_log::write_run(&markdown_run_log::runs_dir(&state), now, &log.text())?;
    Ok(())
}

/// The log's head: when, where, which scry, the command, the caps and the
/// effective `[scry]`, then the steering understood.
fn head(
    a: &RunArgs,
    line: &str,
    project: &Project,
    now: DateTime<Utc>,
    offset: FixedOffset,
) -> String {
    let config = match scry_config(project) {
        Ok(cfg) => config_text(&cfg, a.budget),
        Err(e) => format!("- config: {e}\n"),
    };
    let steering = steering_section(&ScryToml::for_project(project), &project.root);
    format!("{}{config}{steering}", run_text(line, project, now, offset))
}

/// What steering was understood: the file, the context's size, each
/// include, the rule ids and the rendered size against the cap.
fn steering_section(settings: &dyn Settings, root: &Path) -> String {
    let mut s = section("Steering");
    let steering = match settings.steering() {
        Ok(steering) => steering,
        Err(e) => {
            s.push_str(&format!("- error: {e}\n"));
            return s;
        }
    };
    match settings.steering_source() {
        Some(file) => s.push_str(&format!(
            "- file: {}\n",
            file.strip_prefix(root).unwrap_or(&file).display()
        )),
        None => s.push_str(&format!("- none: no {STEERING_FILE}\n")),
    }
    let context = steering.context.trim();
    s.push_str(&format!(
        "- context: {}\n",
        if context.is_empty() {
            "none".to_string()
        } else {
            bytes(context.len())
        }
    ));
    for i in &steering.includes {
        s.push_str(&format!(
            "- include {}: {}\n",
            i.path.display(),
            bytes(i.text.len())
        ));
    }
    let ids: Vec<&str> = steering.rules.iter().map(|r| r.id.as_str()).collect();
    s.push_str(&format!(
        "- rules: {}\n- size: {} of {}\n",
        if ids.is_empty() {
            "none".to_string()
        } else {
            ids.join(", ")
        },
        bytes(steering.size()),
        bytes(STEERING_CAP)
    ));
    s
}

/// One pass over the scope the arguments name, then every open finding of
/// a shown issue type placed in a scope unit, best first.
fn run(
    a: &RunArgs,
    cwd: &Path,
    project: &Project,
    wired: &impl Wiring,
    out: &mut dyn Write,
    log: &StreamingLog,
    width: usize,
) -> Result<i32> {
    check_budget(a.budget)?;
    let cfg = scry_config(project)?;
    let deps = ReviewDeps {
        log,
        ..wired.deps()
    };
    let rows_before = ledger_rows(deps.ledger).len();
    let report = match review_change(project, &request_from(a, cwd), &deps)? {
        ReviewOutcome::Skipped(why) => {
            log.put(&skipped_summary_text(&why));
            log.say(&format!("scry: {why}"))?;
            return Ok(2);
        }
        ReviewOutcome::Ran(r) => r,
    };
    let stored = read_findings(deps.store)?;
    let shown = stored.shown_in_scope(&report.scope, &cfg.issue_types);
    print_findings(out, &shown, a.json, width)?;
    let code = exit_code(shown.len(), &report);
    let new_rows: Vec<Errand> = ledger_rows(deps.ledger)
        .into_iter()
        .skip(rows_before)
        .collect();
    log.put(&summary_text(
        &report, &stored, &shown, &cfg, &new_rows, code,
    ));
    if !shown.is_empty() || report.looked_at > 0 || report.spent > 0.0 {
        say_summary_line(out, log, a.json, &summary_line(shown.len(), &report))?;
    }
    let held = held_back_text(&report, a.budget.is_some());
    if !held.is_empty() {
        log.write_err(held.as_bytes())?;
    }
    Ok(code)
}

/// A `--budget` is a number of dollars, 0 or more.
fn check_budget(budget: Option<f64>) -> Result<()> {
    if let Some(b) = budget
        && (b.is_nan() || b < 0.0)
    {
        anyhow::bail!("--budget {b}: give a number of dollars, 0 or more");
    }
    Ok(())
}

/// The review the arguments ask for: `--since`, the named files (from
/// CWD), or the working tree.
fn request_from(a: &RunArgs, cwd: &Path) -> ReviewRequest {
    let scope = match (&a.since, a.files.is_empty()) {
        (Some(rev), _) => ScopeRequest::Since(rev.clone()),
        (None, false) => ScopeRequest::Files(a.files.iter().map(|f| cwd.join(f)).collect()),
        (None, true) => ScopeRequest::Working,
    };
    ReviewRequest {
        scope,
        all: a.all,
        fresh: a.fresh,
        budget: a.budget,
        caps: None,
    }
}

/// The ledger's rows; none when it cannot be read.
fn ledger_rows(ledger: &dyn Ledger) -> Vec<Errand> {
    ledger.read().map(|r| r.errands).unwrap_or_default()
}

/// SHOWN on OUT: one JSON object a line, or the text form wrapped at WIDTH.
fn print_findings(out: &mut dyn Write, shown: &[&Finding], json: bool, width: usize) -> Result<()> {
    for f in shown {
        if json {
            writeln!(out, "{}", finding_json(f))?;
        } else {
            write!(out, "{}", finding_text(f, width))?;
        }
    }
    Ok(())
}

/// 1 when a finding is shown; else 2 when a call failed or a cap held
/// something back; else 0.
fn exit_code(shown: usize, report: &ReviewReport) -> i32 {
    if shown > 0 {
        1
    } else if !report.failures.is_empty() || report.capped > 0 || report.checks_capped {
        2
    } else {
        0
    }
}

/// The summary line on stdout, or on stderr under `--json` (stdout keeps
/// one object a line).
fn say_summary_line(out: &mut dyn Write, log: &StreamingLog, json: bool, line: &str) -> Result<()> {
    if json {
        log.say(line)?;
    } else {
        writeln!(out, "{line}")?;
    }
    Ok(())
}

// The command's text and the run log's.

/// `1 finding, 1 unit looked at, $0.04`.
fn summary_line(shown: usize, report: &ReviewReport) -> String {
    format!(
        "{}, {} looked at, ${:.2}",
        count(shown, "finding"),
        count(report.looked_at, "unit"),
        report.spent
    )
}

/// The failures and what the caps held back, a line each; BUDGET when the
/// command line gave one.
fn held_back_text(r: &ReviewReport, budget: bool) -> String {
    let mut s = String::new();
    for f in &r.failures {
        s.push_str(&format!("scry: {f}\n"));
    }
    let cap = if budget {
        "the budget or the day's cap"
    } else {
        "the day's cap"
    };
    if r.capped > 0 {
        let they = if r.capped == 1 {
            "it waits"
        } else {
            "they wait"
        };
        s.push_str(&format!(
            "scry: {cap} stopped {}; {they} for the next run\n",
            count(r.capped, "unit")
        ));
    }
    if r.checks_capped {
        s.push_str(&format!(
            "scry: {cap} held back a claim check; its finding stays unverified until the next run\n"
        ));
    }
    s
}

fn run_log_not_written_text(e: &anyhow::Error) -> String {
    format!("scry: the run log was not written: {e:#}")
}

/// The head's Run section: when, where, which scry and the command.
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

/// The caps and the effective `[scry]`, its defaults filled in.
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

/// The log's summary: each finding the pass touched, its status and why it
/// is or is not printed; what was spent, by verb; the failures and caps;
/// the result.
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

/// A line for each finding the pass touched: its status, verdict, and
/// whether it is printed.
fn touched_text(
    r: &ReviewReport,
    stored: &Findings,
    shown: &[&Finding],
    cfg: &ScryConfig,
) -> String {
    if r.touched.is_empty() {
        return "- no finding added or changed\n".into();
    }
    let mut s = String::new();
    for id in &r.touched {
        let Some(f) = stored.findings.iter().find(|f| f.id == *id) else {
            s.push_str(&format!("- {}: not in the store as written\n", id.0));
            continue;
        };
        let in_scope = r.scope.units.iter().any(|u| {
            u.file == f.place.file && f.place.unit.as_deref() == Some(u.unit.name.as_str())
        });
        let verdict = f
            .verdict
            .as_ref()
            .map(|v| format!(" ({}: {})", v.verdict, v.why))
            .unwrap_or_default();
        let why = if shown.iter().any(|g| g.id == f.id) {
            "printed".to_string()
        } else {
            format!("not printed: {}", why_not_printed(f, in_scope, cfg))
        };
        s.push_str(&format!(
            "- {} {} at {}:{}: {}{verdict}; {why}\n",
            f.id.0,
            f.issue_type,
            f.place.file.display(),
            f.place.lines.start,
            status_word(f.status),
        ));
    }
    s
}

/// Why F, touched by the pass, is not printed.
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

/// What the pass charged, and the ledger's new scry rows by verb.
fn spent_text(r: &ReviewReport, new_rows: &[Errand]) -> String {
    let mut by_verb: BTreeMap<&str, (usize, f64)> = BTreeMap::new();
    for e in new_rows.iter().filter(|e| e.product == "scry") {
        let v = by_verb.entry(e.verb.as_str()).or_default();
        v.0 += 1;
        v.1 += e.cost_usd.unwrap_or(0.0);
    }
    let verbs: Vec<String> = by_verb
        .iter()
        .map(|(verb, (n, c))| format!("{verb} {n} for {}", usd(*c)))
        .collect();
    format!(
        "- spent: the pass charged {}; the ledger's new scry rows: {}\n",
        usd(r.spent),
        if verbs.is_empty() {
            "none".into()
        } else {
            verbs.join(", ")
        }
    )
}

/// The failed calls, the capped units and a held-back claim check.
fn failures_and_caps_text(r: &ReviewReport) -> String {
    let mut s = String::new();
    if r.failures.is_empty() {
        s.push_str("- failures: none\n");
    }
    for f in &r.failures {
        s.push_str(&format!("- failure: {f}\n"));
    }
    if r.capped > 0 {
        s.push_str(&format!("- capped: {}\n", count(r.capped, "unit")));
    }
    if r.checks_capped {
        s.push_str("- a claim check was held back by the caps\n");
    }
    s
}
