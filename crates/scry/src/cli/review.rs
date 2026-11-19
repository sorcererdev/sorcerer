//! `scry [FILES]...`: one review over the scope the arguments name, then
//! every open finding of a shown issue type placed in a scope unit, best
//! first; the run log written whatever happened.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Local, Utc};
use sorcerer_core::domain::project::Project;

use super::RunArgs;
use super::format::{finding_json, finding_text};
use crate::adapters::markdown_run_log::{self, RESULT, StreamingLog};
use crate::adapters::scry_toml::{STEERING_FILE, ScryToml};
use crate::adapters::wiring::Wired;
use crate::domain::config::{ScryConfig, scry_config};
use crate::domain::finding::{Finding, Status, status_word};
use crate::domain::findings::Findings;
use crate::domain::log_text::{block, bytes, count, section, usd};
use crate::domain::scope::{Scope, ScopeRequest};
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
pub(super) fn logged<W: Wired>(
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
    let (now, offset) = match &wired {
        Ok(w) => {
            let d = w.deps();
            (d.clock.now(), d.clock.offset())
        }
        Err(_) => (Utc::now(), *Local::now().offset()),
    };
    log.put(&head(a, line, project, now, offset));
    let result = match wired {
        Ok(w) => run(a, cwd, project, &w, out, &log, width),
        Err(e) => Err(e),
    };
    if let Err(e) = &result {
        log.put(&format!(
            "{}- error: {e:#}\n\n{RESULT}could not run, exit 2\n",
            section("Summary")
        ));
    }
    let written = project
        .state_dir()
        .map_err(anyhow::Error::from)
        .and_then(|state| {
            markdown_run_log::write_run(&markdown_run_log::runs_dir(&state), now, &log.text())
                .map_err(anyhow::Error::from)
        });
    if let Err(e) = written {
        log.say(&format!("scry: the run log was not written: {e:#}"))?;
    }
    result
}

/// The log's head: when, where, which scry, the command, the caps and the
/// effective `[scry]`.
fn head(
    a: &RunArgs,
    line: &str,
    project: &Project,
    now: DateTime<Utc>,
    offset: FixedOffset,
) -> String {
    let mut s = format!(
        "# scry run {}\n{}- time: {} (UTC {})\n- project: `{}`\n- scry {}\n- command: `{line}`\n",
        now.format("%Y-%m-%dT%H:%M:%SZ"),
        section("Run"),
        now.with_timezone(&offset).format("%Y-%m-%d %H:%M:%S %:z"),
        now.format("%Y-%m-%d %H:%M:%S"),
        project.root.display(),
        env!("CARGO_PKG_VERSION"),
    );
    match scry_config(project) {
        Ok(cfg) => {
            s.push_str(&format!(
                "- caps: pass none on the command line, day {}, budget {}\n- config ([scry] with the defaults filled in):\n\n",
                usd(cfg.budget_per_day),
                a.budget.map_or("none".into(), usd),
            ));
            let fields = serde_json::to_value(&cfg)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            let text: String = fields.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
            s.push_str(&block(&text));
        }
        Err(e) => s.push_str(&format!("- config: {e}\n")),
    }
    s.push_str(&steering_section(
        &ScryToml::for_project(project),
        &project.root,
    ));
    s
}

/// What steering was understood: the file, the context's size, each
/// include, the rule ids and the rendered size against the cap.
fn steering_section(settings: &dyn Settings, root: &Path) -> String {
    let mut s = section("Steering");
    let st = match settings.steering() {
        Ok(st) => st,
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
    let context = st.context.trim();
    s.push_str(&format!(
        "- context: {}\n",
        if context.is_empty() {
            "none".to_string()
        } else {
            bytes(context.len())
        }
    ));
    for i in &st.includes {
        s.push_str(&format!(
            "- include {}: {}\n",
            i.path.display(),
            bytes(i.text.len())
        ));
    }
    let ids: Vec<&str> = st.rules.iter().map(|r| r.id.as_str()).collect();
    s.push_str(&format!(
        "- rules: {}\n- size: {} of {}\n",
        if ids.is_empty() {
            "none".to_string()
        } else {
            ids.join(", ")
        },
        bytes(st.size()),
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
    wired: &impl Wired,
    out: &mut dyn Write,
    log: &StreamingLog,
    width: usize,
) -> Result<i32> {
    if let Some(b) = a.budget
        && (b.is_nan() || b < 0.0)
    {
        anyhow::bail!("--budget {b}: give a number of dollars, 0 or more");
    }
    let cfg = scry_config(project)?;
    let deps = ReviewDeps {
        log,
        ..wired.deps()
    };
    let scope = match (&a.since, a.files.is_empty()) {
        (Some(rev), _) => ScopeRequest::Since(rev.clone()),
        (None, false) => ScopeRequest::Files(a.files.iter().map(|f| cwd.join(f)).collect()),
        (None, true) => ScopeRequest::Working,
    };
    let req = ReviewRequest {
        scope,
        all: a.all,
        fresh: a.fresh,
        budget: a.budget,
        caps: None,
    };
    let ledger_rows = || deps.ledger.read().map(|r| r.errands).unwrap_or_default();
    let rows_before = ledger_rows().len();
    let report = match review_change(project, &req, &deps)? {
        ReviewOutcome::Skipped(why) => {
            log.put(&format!(
                "{}- skipped: {why}\n\n{RESULT}skipped ({why}), exit 2\n",
                section("Summary")
            ));
            log.say(&format!("scry: {why}"))?;
            return Ok(2);
        }
        ReviewOutcome::Ran(r) => r,
    };
    let stored = read_findings(deps.store)?;
    let scope = &report.scope;
    let shown = stored.shown_in_scope(scope, &cfg.issue_types);
    for f in &shown {
        if a.json {
            writeln!(out, "{}", finding_json(f))?;
        } else {
            write!(out, "{}", finding_text(f, width))?;
        }
    }
    let summary = format!(
        "{}, {} looked at, ${:.2}",
        count(shown.len(), "finding"),
        count(report.looked_at, "unit"),
        report.spent
    );
    let code = if !shown.is_empty() {
        1
    } else if !report.failures.is_empty() || report.capped > 0 || report.checks_capped {
        2
    } else {
        0
    };
    let new_rows: Vec<_> = ledger_rows().into_iter().skip(rows_before).collect();
    log.put(&summary_text(
        &report, &stored, &shown, scope, &cfg, &new_rows, code,
    ));
    if !shown.is_empty() || report.looked_at > 0 || report.spent > 0.0 {
        if a.json {
            log.say(&summary)?;
        } else {
            writeln!(out, "{summary}")?;
        }
    }
    let mut held = Vec::new();
    held_back(&report, a.budget.is_some(), &mut held)?;
    if !held.is_empty() {
        log.write_err(&held)?;
    }
    Ok(code)
}

/// The log's summary: each finding the pass touched, its status and why it
/// is or is not printed; what was spent, by verb; the failures; the exit.
fn summary_text(
    r: &ReviewReport,
    stored: &Findings,
    shown: &[&Finding],
    scope: &Scope,
    cfg: &ScryConfig,
    new_rows: &[sorcerer_core::domain::errand::Errand],
    code: i32,
) -> String {
    let mut s = section("Summary");
    for id in &r.touched {
        let Some(f) = stored.findings.iter().find(|f| f.id == *id) else {
            s.push_str(&format!("- {}: not in the store as written\n", id.0));
            continue;
        };
        let in_scope = scope.units.iter().any(|u| {
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
            let because = match f.status {
                Status::Open if !cfg.issue_types.contains(&f.issue_type) => {
                    format!(
                        "its issue type {} is not in [scry] issue_types",
                        f.issue_type
                    )
                }
                Status::Open if !in_scope => "its unit is not in this run's scope".into(),
                Status::Open => "open".into(),
                Status::Waiting => "not checked yet (a cap or a failed check)".into(),
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
            };
            format!("not printed: {because}")
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
    if r.touched.is_empty() {
        s.push_str("- no finding added or changed\n");
    }
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
    s.push_str(&format!(
        "- spent: the pass charged {}; the ledger's new scry rows: {}\n",
        usd(r.spent),
        if verbs.is_empty() {
            "none".into()
        } else {
            verbs.join(", ")
        }
    ));
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
    s.push_str(&format!(
        "\n{RESULT}{} shown, {} looked at, ${:.2}, exit {code}\n",
        count(shown.len(), "finding"),
        count(r.looked_at, "unit"),
        r.spent
    ));
    s
}

/// The failures and what the caps held back, a line each.
fn held_back(r: &ReviewReport, budget: bool, err: &mut dyn Write) -> Result<()> {
    for f in &r.failures {
        writeln!(err, "scry: {f}")?;
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
        writeln!(
            err,
            "scry: {cap} stopped {}; {they} for the next run",
            count(r.capped, "unit")
        )?;
    }
    if r.checks_capped {
        writeln!(
            err,
            "scry: {cap} held back a claim check; its finding waits for the next run"
        )?;
    }
    Ok(())
}
