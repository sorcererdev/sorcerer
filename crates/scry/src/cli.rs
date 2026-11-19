//! The command line: `scry [FILES]...` runs a pass and prints the open
//! findings in its scope, writing a run log (`scry log` prints it, `-v`
//! streams it); `scry doctor` says whether scry can run here.
//! Exit codes: 0 nothing open in scope, 1 open findings in scope, 2 could
//! not run.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, FixedOffset, Local, NaiveDate, Utc};
use clap::{Args, Parser, Subcommand};
use serde_json::json;
use sorcerer_core::Project;
use sorcerer_core::env::Env;
use sorcerer_core::grammar::GrammarCache;
use sorcerer_core::ledger::Ledger;
use sorcerer_core::project::default_decider;
use sorcerer_core::store::{Snapshot, Store};
use sorcerer_core::unit::Span;
use sorcerer_core::view::{CodebaseView, FilesView};
use sorcerer_models::decide::jev::Jev;
use sorcerer_models::providers::default_provider_for;

use crate::config::scry_config;
use crate::finding::{Finding, Status};
use crate::live::{Live, Wired};
use crate::log::{self, RESULT, count, section, usd};
use crate::pass::{Caps, Outcome, PassDeps, Report, pass};
use crate::scope::{Scope, ScopeWhy, scope_of};
use crate::steering::steering;
use crate::store::{ScryStore, store};

#[derive(Parser, Debug)]
#[command(
    name = "scry",
    version,
    about = "Reads a change and says what it gets wrong",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Files to review (their changed units; with --all, every unit). A file
    /// named like a subcommand is passed as ./list
    files: Vec<PathBuf>,
    /// Every unit of each file in scope, not only the changed ones
    #[arg(long)]
    all: bool,
    /// The files the branch changed since it left REV
    #[arg(long, value_name = "REV", conflicts_with = "files")]
    since: Option<String>,
    /// The findings as JSON, one object a line
    #[arg(long)]
    json: bool,
    /// Look again at units already inspected as they are now
    #[arg(long)]
    fresh: bool,
    /// The most this run may spend, in dollars (the day's cap still holds)
    #[arg(long, value_name = "USD")]
    budget: Option<f64>,
    /// Stream the run log to stderr as the run goes
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// The findings in the store, by status
    List {
        #[arg(long)]
        status: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// What scry spent, a local day a row
    Cost {
        #[arg(long, value_name = "YYYY-MM-DD")]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Stop showing a finding, here or everywhere
    Dismiss {
        id: String,
        #[arg(long)]
        everywhere: bool,
    },
    /// A finding in full
    Why { id: String },
    /// A run's log: the latest, or the Nth latest
    Log {
        /// 1 is the latest run, 2 the one before, …
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        n: Option<u32>,
        /// The logs kept, newest first: file, time, result
        #[arg(long, conflicts_with_all = ["n", "path"])]
        list: bool,
        /// The log's path only
        #[arg(long)]
        path: bool,
    },
    /// Whether scry can run here: provider, Jev, languages, steering, store, ledger
    Doctor,
}

/// The binary's entry: the process's directory, the live wiring, and the
/// terminal's width (100 when stdout is not a terminal).
pub fn main(args: Vec<OsString>, env: &dyn Env, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(err, "scry: the current directory: {e}");
            return 2;
        }
    };
    let width = if std::io::stdout().is_terminal() {
        terminal_size::terminal_size()
            .map(|(w, _)| w.0 as usize)
            .unwrap_or(100)
    } else {
        100
    };
    main_with(args, &cwd, env, out, err, width, |p| Live::open(p, env))
}

/// [`main`] with the directory, width and wiring given (the tests' doubles).
#[doc(hidden)]
pub fn main_with<W: Wired>(
    args: Vec<OsString>,
    cwd: &Path,
    env: &dyn Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
    width: usize,
    wire: impl Fn(&Project) -> Result<W>,
) -> i32 {
    let line = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    let cli = match Cli::try_parse_from(args) {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let text = e.render().to_string();
            let _ = if e.use_stderr() {
                err.write_all(text.as_bytes())
            } else {
                out.write_all(text.as_bytes())
            };
            return code;
        }
    };
    let code = match go(cli, &line, cwd, env, out, err, width, wire) {
        Ok(code) => code,
        Err(e) => {
            let _ = writeln!(err, "scry: {e:#}");
            2
        }
    };
    let _ = out.flush();
    let _ = err.flush();
    code
}

#[allow(clippy::too_many_arguments)]
fn go<W: Wired>(
    cli: Cli,
    line: &str,
    cwd: &Path,
    env: &dyn Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
    width: usize,
    wire: impl Fn(&Project) -> Result<W>,
) -> Result<i32> {
    let project = Project::open(cwd)?;
    match cli.cmd {
        None => {
            // off is not a failure, and needs no wiring (no Jev key, no
            // provider): a pre-commit hook lets the commit through
            if !scry_config(&project)?.enabled {
                writeln!(err, "scry: off ([scry] enabled = false)")?;
                return Ok(0);
            }
            logged(&cli.run, line, cwd, &project, wire, out, err, width)
        }
        Some(Cmd::Doctor) => Ok(doctor(&project, env, &GrammarCache::user(env), out)?),
        Some(Cmd::List { status, json }) => list(&project, &status, json, out),
        Some(Cmd::Cost { since, json }) => {
            let (_, offset) = clock_of(&project, &wire);
            cost(&project, since.as_deref(), offset, json, out, err)
        }
        Some(Cmd::Dismiss { id, everywhere }) => {
            let (now, _) = clock_of(&project, &wire);
            dismiss(&project, &id, everywhere, now, out, err)
        }
        Some(Cmd::Why { id }) => why(&project, &id, out, err),
        Some(Cmd::Log { n, list, path }) => {
            let (_, offset) = clock_of(&project, &wire);
            show_log(&project, n.unwrap_or(1), list, path, offset, out, err)
        }
    }
}

/// The run log as a run writes it: kept whole for the file, and with
/// `-v` streamed to stderr as it grows. Stderr's other lines go through
/// it too, so the two never interleave out of order.
struct RunLog<'w> {
    text: RefCell<String>,
    err: RefCell<&'w mut dyn Write>,
    verbose: bool,
}

impl RunLog<'_> {
    fn put(&self, s: &str) {
        self.text.borrow_mut().push_str(s);
        if self.verbose {
            let _ = self.err.borrow_mut().write_all(s.as_bytes());
        }
    }

    /// One line on stderr, not in the log.
    fn say(&self, s: &str) -> std::io::Result<()> {
        writeln!(self.err.borrow_mut(), "{s}")
    }
}

/// A run, logged: the head, the run itself, then the summary's last line;
/// the log is written under `state/scry/runs/` whatever the run did, and a
/// failure to write it is one line on stderr.
#[allow(clippy::too_many_arguments)]
fn logged<W: Wired>(
    a: &RunArgs,
    line: &str,
    cwd: &Path,
    project: &Project,
    wire: impl Fn(&Project) -> Result<W>,
    out: &mut dyn Write,
    err: &mut dyn Write,
    width: usize,
) -> Result<i32> {
    let log = RunLog {
        text: RefCell::new(String::new()),
        err: RefCell::new(err),
        verbose: a.verbose,
    };
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
            log::write_run(&log::runs_dir(&state), now, &log.text.borrow())
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
                usd(cfg.day_cost),
                a.budget.map_or("none".into(), usd),
            ));
            let fields = serde_json::to_value(&cfg)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            let text: String = fields.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
            s.push_str(&log::block(&text));
        }
        Err(e) => s.push_str(&format!("- config: {e}\n")),
    }
    s
}

/// `scry log`: the Nth latest run's log, the list of them, or a path.
fn show_log(
    project: &Project,
    n: u32,
    list: bool,
    path: bool,
    offset: FixedOffset,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let dir = log::runs_dir(&project.state);
    let logs = log::runs(&dir);
    if logs.is_empty() {
        writeln!(
            err,
            "scry: no run log yet (a run writes one in {})",
            dir.display()
        )?;
        return Ok(2);
    }
    if list {
        let rows: Vec<Vec<String>> = logs
            .iter()
            .map(|p| {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let time = log::run_time(&name)
                    .map(|t| {
                        t.with_timezone(&offset)
                            .format("%Y-%m-%d %H:%M:%S %:z")
                            .to_string()
                    })
                    .unwrap_or_default();
                let text = std::fs::read_to_string(p).unwrap_or_default();
                let result = log::result_of(&text)
                    .unwrap_or("(no result line)")
                    .to_string();
                vec![name, time, result]
            })
            .collect();
        write!(out, "{}", table(&["log", "time", "result"], &rows))?;
        return Ok(0);
    }
    let Some(p) = logs.get(n as usize - 1) else {
        writeln!(
            err,
            "scry: no run log {n}: {} kept",
            count(logs.len(), "log")
        )?;
        return Ok(2);
    };
    if path {
        writeln!(out, "{}", p.display())?;
    } else {
        let text =
            std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
        out.write_all(text.as_bytes())?;
    }
    Ok(0)
}

/// The time and local offset: the wiring's clock when it opens (the tests'
/// doubles), else the system's. These commands need no Jev, so a wiring
/// that does not open is not an error here.
fn clock_of<W: Wired>(
    project: &Project,
    wire: &impl Fn(&Project) -> Result<W>,
) -> (DateTime<Utc>, FixedOffset) {
    match wire(project) {
        Ok(w) => {
            let d = w.deps();
            (d.clock.now(), d.clock.offset())
        }
        Err(_) => (Utc::now(), *Local::now().offset()),
    }
}

fn read_store(project: &Project) -> Result<ScryStore> {
    Ok(match store(project)?.read()? {
        Snapshot::Current(s) => s,
        Snapshot::Absent => ScryStore::default(),
        Snapshot::Unreadable(why) => anyhow::bail!("the finding store cannot be read ({why})"),
    })
}

fn status_name(s: Status) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Open first, the order of the status type.
fn status_rank(s: Status) -> u8 {
    use Status::*;
    match s {
        Open => 0,
        Waiting => 1,
        Withheld => 2,
        Refuted => 3,
        Unsure => 4,
        Elsewhere => 5,
        DismissedHere => 6,
        DismissedEverywhere => 7,
        Resolved => 8,
        Gone => 9,
    }
}

/// Columns padded to their widest cell, two spaces apart, no trailing space.
fn table(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = header.iter().map(|h| h.chars().count()).collect();
    for r in rows {
        for (w, c) in widths.iter_mut().zip(r) {
            *w = (*w).max(c.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let padded: Vec<String> = cells
            .iter()
            .zip(&widths)
            .map(|(c, w)| format!("{c:<w$}"))
            .collect();
        format!("{}\n", padded.join("  ").trim_end())
    };
    let mut out = line(header.to_vec());
    for r in rows {
        out.push_str(&line(r.iter().map(String::as_str).collect()));
    }
    out
}

fn cut_to(n: usize, s: &str) -> String {
    if s.chars().count() > n {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    } else {
        s.to_string()
    }
}

/// The findings of STATUSES (all when none), open first, then file and line.
fn list(project: &Project, statuses: &[String], json: bool, out: &mut dyn Write) -> Result<i32> {
    let st = read_store(project)?;
    let mut fs: Vec<&Finding> = st
        .findings
        .iter()
        .filter(|f| statuses.is_empty() || statuses.contains(&status_name(f.status)))
        .collect();
    fs.sort_by(|a, b| {
        (status_rank(a.status), &a.place.file, a.place.lines.start).cmp(&(
            status_rank(b.status),
            &b.place.file,
            b.place.lines.start,
        ))
    });
    if json {
        let v: Vec<_> = fs
            .iter()
            .map(|f| {
                json!({
                    "id": f.id.0,
                    "status": status_name(f.status),
                    "kind": f.kind,
                    "file": f.place.file,
                    "line": f.place.lines.start,
                    "rule": f.rule,
                    "statement": f.statement,
                    "keep": f.scores.keep,
                    "verdict": f.verdict.as_ref().map(|v| v.verdict.clone()),
                    "seen": f.seen,
                })
            })
            .collect();
        writeln!(out, "{}", json!({ "findings": v }))?;
    } else {
        let rows: Vec<Vec<String>> = fs
            .iter()
            .map(|f| {
                vec![
                    status_name(f.status),
                    f.kind.clone(),
                    f.place.file.display().to_string(),
                    f.place.lines.start.to_string(),
                    f.rule.clone().unwrap_or_else(|| "-".into()),
                    cut_to(80, &f.statement),
                ]
            })
            .collect();
        write!(
            out,
            "{}",
            table(
                &["status", "kind", "file", "line", "rule", "statement"],
                &rows
            )
        )?;
    }
    Ok(0)
}

/// What scry spent, a local day (in OFFSET) a row, from SINCE on; `decide`
/// errands count into the total only.
fn cost(
    project: &Project,
    since: Option<&str>,
    offset: FixedOffset,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let since = since
        .map(|s| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| anyhow::anyhow!("--since {s}: give a day as YYYY-MM-DD"))
        })
        .transpose()?;
    let rows = Ledger::for_project(project)?.read()?.errands;
    // (finds, checks, find $, check $, total $)
    let mut days: BTreeMap<NaiveDate, (usize, usize, f64, f64, f64)> = BTreeMap::new();
    for e in rows.iter().filter(|e| e.product == "scry") {
        let day = e.at.with_timezone(&offset).date_naive();
        if since.is_some_and(|s| day < s) {
            continue;
        }
        let d = days.entry(day).or_default();
        let c = e.cost_usd.unwrap_or(0.0);
        match e.verb.as_str() {
            "find" => {
                d.0 += 1;
                d.2 += c;
            }
            "check" => {
                d.1 += 1;
                d.3 += c;
            }
            _ => {}
        }
        d.4 += c;
    }
    // a store that does not read is said, not counted as nothing waiting
    let waiting: Option<usize> = match read_store(project) {
        Ok(s) => Some(s.capped.values().map(|u| u.len()).sum()),
        Err(e) => {
            writeln!(err, "scry: {e}")?;
            None
        }
    };
    if json {
        let v: Vec<_> = days
            .iter()
            .map(|(day, d)| {
                json!({
                    "day": day.to_string(),
                    "finds": d.0,
                    "checks": d.1,
                    "find_cost": d.2,
                    "check_cost": d.3,
                    "total_cost": d.4,
                })
            })
            .collect();
        writeln!(out, "{}", json!({ "days": v, "waiting_units": waiting }))?;
    } else {
        let t: Vec<Vec<String>> = days
            .iter()
            .map(|(day, d)| {
                vec![
                    day.to_string(),
                    d.0.to_string(),
                    d.1.to_string(),
                    format!("{:.2}", d.2),
                    format!("{:.2}", d.3),
                    format!("{:.2}", d.4),
                ]
            })
            .collect();
        write!(
            out,
            "{}",
            table(
                &["day", "finds", "checks", "find $", "check $", "total $"],
                &t
            )
        )?;
        if let Some(n) = waiting {
            writeln!(out, "waiting on the cap: {}", count(n, "unit"))?;
        }
    }
    Ok(0)
}

fn dismiss(
    project: &Project,
    id: &str,
    everywhere: bool,
    at: DateTime<Utc>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let outcome = store(project)?.update(|s| {
        s.dismiss(id, everywhere, at)
            .map(|()| s.get(id).map(|f| f.id.0.clone()).unwrap_or_default())
    })?;
    match outcome {
        Ok(full) => {
            writeln!(
                out,
                "dismissed {full} ({})",
                if everywhere { "everywhere" } else { "here" }
            )?;
            Ok(0)
        }
        Err(why) => {
            writeln!(err, "scry: {why}")?;
            Ok(2)
        }
    }
}

/// A finding in full, as Markdown.
fn why(project: &Project, id: &str, out: &mut dyn Write, err: &mut dyn Write) -> Result<i32> {
    let st = read_store(project)?;
    let Some(f) = st.get(id) else {
        let n = st
            .findings
            .iter()
            .filter(|f| id.len() >= 4 && f.id.0.starts_with(id))
            .count();
        let msg = if n > 1 {
            format!("{id} matches more than one finding")
        } else {
            format!("no finding {id}")
        };
        writeln!(err, "scry: {msg}")?;
        return Ok(2);
    };
    let file = f.place.file.display();
    let lines = lines_text(f.place.lines.start, f.place.lines.end);
    writeln!(out, "# {}: {file}:{lines}\n\n{}\n", f.kind, f.statement)?;
    if let Some(r) = &f.rule {
        let st = steering(project)?;
        let name = st
            .rule_text(r)
            .map(|_| r.as_str())
            .or_else(|| st.resolve_rule(r))
            .unwrap_or(r.as_str());
        writeln!(out, "Rule: {name}")?;
        if let Some(t) = st.rule_text(name) {
            writeln!(out, "{}", t.trim())?;
        }
        writeln!(out)?;
    }
    match (&f.place.unit, f.place.unit_start) {
        (Some(u), Some(at)) => writeln!(out, "Unit: {u} (starts at {file}:{at})\n")?,
        (Some(u), None) => writeln!(out, "Unit: {u}\n")?,
        _ => {}
    }
    if !f.evidence.is_empty() {
        // a citation the checks never saw may name any path: read only
        // what the fences allow, inside the root
        let fences = sorcerer_core::fences::Fences::for_project(project)?;
        writeln!(out, "Evidence:")?;
        for e in &f.evidence {
            writeln!(
                out,
                "{}:{}",
                e.file.display(),
                lines_text(e.lines.start, e.lines.end)
            )?;
            if e.file.is_absolute() || !fences.allows(&e.file) {
                writeln!(out, "(not shown: fenced or outside the project)")?;
                continue;
            }
            match std::fs::read_to_string(project.root.join(&e.file)) {
                Ok(text) => writeln!(out, "{}", cited_range(&text, e.lines))?,
                Err(_) => writeln!(out, "(the file cannot be read)")?,
            }
        }
        writeln!(out)?;
    }
    if let Some(v) = &f.verdict {
        writeln!(out, "Checked: {}: {}", v.verdict, v.why)?;
        if !v.cited.is_empty() {
            writeln!(out, "Cited by the check:")?;
            for c in &v.cited {
                writeln!(out, "{}: {}", c.label(), c.outcome())?;
            }
        }
    }
    if let Some(k) = f.scores.keep {
        writeln!(out, "Keep: {k:.2}")?;
    }
    writeln!(
        out,
        "Seen: {} since {}\n",
        if f.seen == 1 {
            "1 time".to_string()
        } else {
            format!("{} times", f.seen)
        },
        f.first.format("%Y-%m-%d")
    )?;
    writeln!(out, "History:")?;
    for (at, s) in &f.history {
        writeln!(
            out,
            "{}  {}",
            at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            status_name(*s)
        )?;
    }
    Ok(0)
}

/// One pass over the scope the arguments name, then every open finding of
/// a shown kind placed in a scope unit, best first.
fn run(
    a: &RunArgs,
    cwd: &Path,
    project: &Project,
    wired: &impl Wired,
    out: &mut dyn Write,
    log: &RunLog,
    width: usize,
) -> Result<i32> {
    if let Some(b) = a.budget
        && (b.is_nan() || b < 0.0)
    {
        anyhow::bail!("--budget {b}: give a number of dollars, 0 or more");
    }
    let cfg = scry_config(project)?;
    let trace = |s: &str| log.put(s);
    let deps = PassDeps {
        trace: &trace,
        ..wired.deps()
    };
    let why = match (&a.since, a.files.is_empty()) {
        (Some(rev), _) => ScopeWhy::Since(rev.clone()),
        (None, false) => ScopeWhy::Named(a.files.iter().map(|f| cwd.join(f)).collect()),
        (None, true) => ScopeWhy::Working,
    };
    let scope = scope_of(project, deps.view, why, a.all, &cfg)?;
    log.put(&log::scope_text(&scope));
    let caps = Caps {
        pass: None,
        day: cfg.day_cost,
        budget: a.budget,
    };
    let ledger_rows = || {
        Ledger::for_project(project)
            .and_then(|l| l.read())
            .map(|r| r.errands)
            .unwrap_or_default()
    };
    let rows_before = ledger_rows().len();
    let report = match pass(project, &scope, &caps, a.fresh, &deps, &wired.cancel())? {
        Outcome::Skipped(why) => {
            log.put(&format!(
                "{}- skipped: {why}\n\n{RESULT}skipped ({why}), exit 2\n",
                section("Summary")
            ));
            log.say(&format!("scry: {why}"))?;
            return Ok(2);
        }
        Outcome::Ran(r) => r,
    };
    let stored = match store(project)?.read()? {
        Snapshot::Current(s) => s,
        Snapshot::Absent => ScryStore::default(),
        Snapshot::Unreadable(why) => anyhow::bail!("the finding store cannot be read ({why})"),
    };
    let shown = shown_in_scope(&stored, &scope, &cfg.kinds);
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
        &report, &stored, &shown, &scope, &cfg, &new_rows, code,
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
        log.err.borrow_mut().write_all(&held)?;
    }
    Ok(code)
}

/// The log's summary: each finding the pass touched, its status and why it
/// is or is not printed; what was spent, by verb; the failures; the exit.
fn summary_text(
    r: &Report,
    stored: &ScryStore,
    shown: &[&Finding],
    scope: &Scope,
    cfg: &crate::config::ScryConfig,
    new_rows: &[sorcerer_core::ledger::Errand],
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
                Status::Open if !cfg.kinds.contains(&f.kind) => {
                    format!("its kind {} is not in [scry] kinds", f.kind)
                }
                Status::Open if !in_scope => "its unit is not in this run's scope".into(),
                Status::Open => "open".into(),
                Status::Waiting => "not checked yet (a cap or a failed check)".into(),
                Status::Withheld => format!(
                    "judged a nitpick (keep {:.2} under the cut {:.2})",
                    f.scores.keep.unwrap_or(0.0),
                    cfg.cut
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
            f.kind,
            f.place.file.display(),
            f.place.lines.start,
            crate::pass::status_word(f.status),
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
fn held_back(r: &Report, budget: bool, err: &mut dyn Write) -> Result<()> {
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

/// Open findings of KINDS whose place is a unit of SCOPE, best keep first.
fn shown_in_scope<'s>(s: &'s ScryStore, scope: &Scope, kinds: &[String]) -> Vec<&'s Finding> {
    let mut v: Vec<&Finding> = s
        .findings
        .iter()
        .filter(|f| {
            f.status == Status::Open
                && kinds.contains(&f.kind)
                && scope.units.iter().any(|u| {
                    u.file == f.place.file && f.place.unit.as_deref() == Some(u.unit.name.as_str())
                })
        })
        .collect();
    v.sort_by(|a, b| {
        let (ka, kb) = (a.scores.keep.unwrap_or(0.0), b.scores.keep.unwrap_or(0.0));
        kb.total_cmp(&ka)
    });
    v
}

/// The cited lines whole, two either side, numbered as the claim prompt
/// numbers them; a range past the end stops at the last line.
fn cited_range(text: &str, lines: Span) -> String {
    let ls: Vec<&str> = text.lines().collect();
    let lo = lines.start.saturating_sub(2).max(1) as usize;
    let hi = (lines.end.max(lines.start) as usize + 2).min(ls.len());
    (lo..=hi)
        .filter_map(|n| ls.get(n - 1).map(|l| format!("{n:>4} {l}")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn lines_text(a: u32, z: u32) -> String {
    if a == z {
        a.to_string()
    } else {
        format!("{a}-{z}")
    }
}

/// `file:line  kind  statement  (keep)`, the statement wrapped to WIDTH
/// under itself, then the evidence and the `scry why` line.
fn finding_text(f: &Finding, width: usize) -> String {
    let head = format!(
        "{}:{}  {}  ",
        f.place.file.display(),
        f.place.lines.start,
        f.kind
    );
    let indent = head.chars().count();
    let room = width.saturating_sub(indent).max(30);
    let tag = format!("({:.2})", f.scores.keep.unwrap_or(0.0));
    let mut lines: Vec<String> = vec![String::new()];
    for word in f.statement.split_whitespace() {
        let last = lines.last_mut().expect("one line at least");
        if last.is_empty() {
            last.push_str(word);
        } else if last.chars().count() + 1 + word.chars().count() <= room {
            last.push(' ');
            last.push_str(word);
        } else {
            lines.push(word.to_string());
        }
    }
    let last = lines.last_mut().expect("one line at least");
    if last.chars().count() + 2 + tag.len() <= room {
        last.push_str("  ");
        last.push_str(&tag);
    } else {
        lines.push(tag);
    }
    let mut text = head;
    text.push_str(&lines.join(&format!("\n{}", " ".repeat(indent))));
    text.push('\n');
    if !f.evidence.is_empty() {
        let cited: Vec<String> = f
            .evidence
            .iter()
            .map(|e| {
                format!(
                    "{}:{}",
                    e.file.display(),
                    lines_text(e.lines.start, e.lines.end)
                )
            })
            .collect();
        text.push_str(&format!("  evidence: {}\n", cited.join(", ")));
    }
    text.push_str(&format!("  scry why {}\n", f.id.0));
    text
}

fn finding_json(f: &Finding) -> serde_json::Value {
    json!({
        "id": f.id.0,
        "kind": f.kind,
        "rule": f.rule,
        "statement": f.statement,
        "file": f.place.file,
        "lines": [f.place.lines.start, f.place.lines.end],
        "unit": f.place.unit,
        "evidence": f.evidence.iter().map(|e| json!({
            "file": e.file,
            "lines": [e.lines.start, e.lines.end],
        })).collect::<Vec<_>>(),
        "keep": f.scores.keep,
        "verdict": f.verdict.as_ref().map(|v| v.verdict.clone()),
        "status": f.status,
    })
}

/// Prints each doctor row; 2 when any failed.
fn doctor(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
    out: &mut dyn Write,
) -> Result<i32> {
    let rows = doctor_rows(project, env, cache);
    let mut code = 0;
    for (name, row) in rows {
        match row {
            Ok(s) => writeln!(out, "{name:<10} ok    {s}")?,
            Err(s) => {
                code = 2;
                writeln!(out, "{name:<10} FAIL  {s}")?
            }
        }
    }
    Ok(code)
}

/// Whether scry can run in PROJECT, a row each: `provider`, `jev`,
/// `languages` (each language the project's files use loads), `steering`,
/// `store`, `ledger`. Reads only; writes nothing.
pub fn doctor_rows(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
) -> Vec<(String, Result<String, String>)> {
    let provider = default_provider_for(project, env)
        .map(|(_, c)| format!("{} ({})", c.name, c.why))
        .map_err(|e| e.to_string());
    let jev = match Jev::from_env(&project.config.decide, env, None) {
        Ok(_) => Ok(default_decider(&project.config.decide, env)
            .map(|c| format!("{} ({})", c.name, c.why))
            .unwrap_or_else(|| "jev".into())),
        Err(e) => Err(format!("{e}; scry does not run without Jev")),
    };
    let steering_row = steering(project)
        .map(|s| {
            let file = if project.root.join(".sorcerer/scry.md").exists() {
                "with .sorcerer/scry.md"
            } else {
                "no .sorcerer/scry.md (the shipped text only)"
            };
            format!("{}, {file}", count(s.rules.len(), "rule"))
        })
        .map_err(|e| e.to_string());
    let store_row = {
        let st: Store<ScryStore> = Store::at(project.state.join("scry/findings.json"));
        match st.read() {
            Ok(Snapshot::Absent) => Ok("no findings yet".into()),
            Ok(Snapshot::Current(s)) => {
                let open = s
                    .findings
                    .iter()
                    .filter(|f| f.status == Status::Open)
                    .count();
                Ok(format!(
                    "{}, {open} open",
                    count(s.findings.len(), "finding")
                ))
            }
            Ok(Snapshot::Unreadable(why)) => Err(format!(
                "{} cannot be read ({why}); scry does not run until it is fixed or moved aside",
                st.path().display()
            )),
            Err(e) => Err(e.to_string()),
        }
    };
    let ledger_row = match Ledger::at(project.state.join("ledger.jsonl")).read() {
        Ok(rows) => {
            let scry = rows.errands.iter().filter(|e| e.product == "scry").count();
            let mut s = format!(
                "{}, {scry} of them scry's",
                count(rows.errands.len(), "row")
            );
            if rows.skipped > 0 {
                s.push_str(&format!("; {} did not read", count(rows.skipped, "line")));
            }
            Ok(s)
        }
        Err(e) => Err(e.to_string()),
    };
    vec![
        ("provider".into(), provider),
        ("jev".into(), jev),
        ("languages".into(), languages_row(project, cache)),
        ("steering".into(), steering_row),
        ("store".into(), store_row),
        ("ledger".into(), ledger_row),
    ]
}

/// The project's languages (`[languages] names`, else those covering a
/// file in the project), each loaded through CACHE.
fn languages_row(project: &Project, cache: &GrammarCache) -> Result<String, String> {
    let names: Vec<String> = if project.config.languages.is_empty() {
        let view = FilesView::new(std::sync::Arc::new(project.clone()), cache.clone());
        let files = view.glob("**").map_err(|e| e.to_string())?;
        let mut used: Vec<String> = files
            .iter()
            .filter_map(|f| project.languages.for_path(f).map(|l| l.name.clone()))
            .collect();
        used.sort();
        used.dedup();
        used
    } else {
        project.config.languages.clone()
    };
    if names.is_empty() {
        return Ok("no file here is in a language sorcerer knows".into());
    }
    let failed: Vec<String> = names
        .iter()
        .filter_map(|n| match project.languages.load(n, cache) {
            Ok(_) => None,
            Err(e) => Some(format!("{n}: {e}")),
        })
        .collect();
    if failed.is_empty() {
        Ok(format!("{} load", names.join(", ")))
    } else {
        Err(failed.join("; "))
    }
}
