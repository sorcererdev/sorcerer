//! The command line: `scry [FILES]...` runs a pass and prints the open
//! findings in its scope; `scry doctor` says whether scry can run here.
//! Exit codes: 0 nothing open in scope, 1 open findings in scope, 2 could
//! not run.

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use serde_json::json;
use sorcerer_core::Project;
use sorcerer_core::env::Env;
use sorcerer_core::grammar::GrammarCache;
use sorcerer_core::ledger::Ledger;
use sorcerer_core::project::default_decider;
use sorcerer_core::store::{Snapshot, Store};
use sorcerer_core::view::{CodebaseView, FilesView};
use sorcerer_models::decide::jev::Jev;
use sorcerer_models::providers::default_provider_for;

use crate::config::scry_config;
use crate::finding::{Finding, Status};
use crate::live::{Live, Wired};
use crate::pass::{Caps, Outcome, Report, pass};
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
    let code = match go(cli, cwd, env, out, err, width, wire) {
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

fn go<W: Wired>(
    cli: Cli,
    cwd: &Path,
    env: &dyn Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
    width: usize,
    wire: impl Fn(&Project) -> Result<W>,
) -> Result<i32> {
    let project = Project::open(cwd)?;
    match cli.cmd {
        None => run(&cli.run, cwd, &project, &wire(&project)?, out, err, width),
        Some(Cmd::Doctor) => Ok(doctor(&project, env, &GrammarCache::user(env), out)?),
        Some(Cmd::List { .. }) => not_yet("list", err),
        Some(Cmd::Cost { .. }) => not_yet("cost", err),
        Some(Cmd::Dismiss { .. }) => not_yet("dismiss", err),
        Some(Cmd::Why { .. }) => not_yet("why", err),
    }
}

fn not_yet(cmd: &str, err: &mut dyn Write) -> Result<i32> {
    writeln!(err, "scry: `scry {cmd}` is not built yet")?;
    Ok(2)
}

/// One pass over the scope the arguments name, then every open finding of
/// a shown kind placed in a scope unit, best first.
fn run(
    a: &RunArgs,
    cwd: &Path,
    project: &Project,
    wired: &impl Wired,
    out: &mut dyn Write,
    err: &mut dyn Write,
    width: usize,
) -> Result<i32> {
    if let Some(b) = a.budget
        && (b.is_nan() || b < 0.0)
    {
        anyhow::bail!("--budget {b}: give a number of dollars, 0 or more");
    }
    let cfg = scry_config(project)?;
    let deps = wired.deps();
    let why = match (&a.since, a.files.is_empty()) {
        (Some(rev), _) => ScopeWhy::Since(rev.clone()),
        (None, false) => ScopeWhy::Named(a.files.iter().map(|f| cwd.join(f)).collect()),
        (None, true) => ScopeWhy::Working,
    };
    let scope = scope_of(project, deps.view, why, a.all, &cfg)?;
    let caps = Caps {
        pass: None,
        day: cfg.day_cost,
        budget: a.budget,
    };
    let report = match pass(project, &scope, &caps, a.fresh, &deps, &wired.cancel())? {
        Outcome::Skipped(why) => {
            writeln!(err, "scry: {why}")?;
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
    if !shown.is_empty() || report.looked_at > 0 || report.spent > 0.0 {
        if a.json {
            writeln!(err, "{summary}")?;
        } else {
            writeln!(out, "{summary}")?;
        }
    }
    held_back(&report, a.budget.is_some(), err)?;
    Ok(if !shown.is_empty() {
        1
    } else if !report.failures.is_empty() || report.capped > 0 || report.checks_capped {
        2
    } else {
        0
    })
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

fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
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
