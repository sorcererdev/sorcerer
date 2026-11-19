//! The run log: what a run did, step by step, as Markdown. The pass writes
//! its part through the `trace` in `PassDeps` (the find calls, each
//! finding's path, the rounds, what it remembered); the command line adds
//! the run's head, the scope and the summary, streams it all to stderr with
//! `-v`, and keeps it under `state/scry/runs/`, the newest [`KEEP`].

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use crate::finding::StoredCitation;
use crate::scope::{Scope, ScopeWhy};

/// Where a pass writes its trace.
pub type Trace<'a> = &'a dyn Fn(&str);

/// The trace that writes nothing (the gate, and tests that do not read it).
pub fn quiet(_: &str) {}

/// How many run logs are kept.
pub const KEEP: usize = 20;

/// The summary line's lead: `scry log --list` prints what follows it.
pub const RESULT: &str = "**Result:** ";

/// A level-two heading, a blank line either side.
pub fn section(title: &str) -> String {
    format!("\n## {title}\n\n")
}

/// A level-three heading, a blank line either side.
pub fn sub(title: &str) -> String {
    format!("\n### {title}\n\n")
}

/// TEXT whole in a fenced block whose fence is longer than any run of
/// backticks inside it.
pub fn block(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    let nl = if text.ends_with('\n') { "" } else { "\n" };
    format!("{fence}text\n{text}{nl}{fence}\n\n")
}

/// A dollar amount to the tenth of a cent (a negative zero is zero).
pub fn usd(x: f64) -> String {
    format!("${:.3}", if x == 0.0 { 0.0 } else { x })
}

/// `1 unit`, `2 units`.
pub fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// `` `path` `unit` line N: confirmed `` (or `not found`).
pub fn citation(c: &StoredCitation) -> String {
    let mut s = format!("`{}`", c.path);
    if let Some(u) = &c.unit {
        s.push_str(&format!(" `{u}`"));
    }
    if let Some(l) = c.line {
        s.push_str(&format!(" line {l}"));
    }
    let said = if c.confirmed {
        "confirmed"
    } else {
        "not found"
    };
    format!("{s}: {said}")
}

/// The scope's section: why the pass runs, the base, each file in scope
/// with its units, and each file left out with why.
pub fn scope_text(scope: &Scope) -> String {
    let mut s = section("Scope");
    let why = match &scope.why {
        ScopeWhy::Saved(p) => format!("the saved file `{}`", p.display()),
        ScopeWhy::Named(ps) => format!(
            "the files named: {}",
            ps.iter()
                .map(|p| format!("`{}`", p.display()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ScopeWhy::Since(rev) => format!("the files changed since `{rev}`"),
        ScopeWhy::Working => "the working tree".into(),
    };
    s.push_str(&format!("- scope: {why}\n"));
    let base = if scope.base.is_empty() {
        "none (no commit yet: every file is new)".to_string()
    } else {
        format!("`{}`", scope.base)
    };
    s.push_str(&format!("- base: {base}\n"));
    for f in &scope.files {
        if f.deleted {
            s.push_str(&format!(
                "- `{}`: in, deleted (its findings are gone)\n",
                f.file.display()
            ));
            continue;
        }
        let units: Vec<String> = scope
            .units
            .iter()
            .filter(|u| u.file == f.file)
            .map(|u| {
                format!(
                    "`{}` ({}-{})",
                    u.unit.name, u.unit.span.start, u.unit.span.end
                )
            })
            .collect();
        let what = if units.is_empty() {
            "no unit added or changed".to_string()
        } else {
            format!("added or changed: {}", units.join(", "))
        };
        s.push_str(&format!("- `{}`: in; {what}\n", f.file.display()));
    }
    for (file, why) in &scope.skipped {
        s.push_str(&format!("- `{}`: skipped ({why})\n", file.display()));
    }
    s
}

/// The run logs' directory under PROJECT's state.
pub fn runs_dir(state: &Path) -> PathBuf {
    state.join("scry").join("runs")
}

/// Writes TEXT as a new log in DIR named for AT (`2026-11-18T23-00-00Z.md`,
/// then `-2`, `-3` on a clash), then deletes all but the newest KEEP.
pub fn write_run(dir: &Path, at: DateTime<Utc>, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = at.format("%Y-%m-%dT%H-%M-%SZ").to_string();
    let mut n = 1;
    let path = loop {
        let name = if n == 1 {
            format!("{stamp}.md")
        } else {
            format!("{stamp}-{n}.md")
        };
        let p = dir.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p)
        {
            Ok(mut f) => {
                f.write_all(text.as_bytes())?;
                break p;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    for old in runs(dir).into_iter().skip(KEEP) {
        std::fs::remove_file(old)?;
    }
    Ok(path)
}

/// The run logs in DIR, newest first (by the time in the name, then the
/// clash counter). Other files are not logs and are left alone.
pub fn runs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut logs: Vec<((String, u32), PathBuf)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            run_key(&name).map(|k| (k, e.path()))
        })
        .collect();
    logs.sort_by(|a, b| b.0.cmp(&a.0));
    logs.into_iter().map(|(_, p)| p).collect()
}

/// A log's name as (UTC stamp, clash counter): `…Z.md` is 1, `…Z-3.md` is 3.
pub fn run_key(name: &str) -> Option<(String, u32)> {
    let stem = name.strip_suffix(".md")?;
    let (stamp, n) = match stem.split_once("Z-") {
        Some((s, n)) => (format!("{s}Z"), n.parse().ok()?),
        None => (stem.to_string(), 1),
    };
    let ok = stamp.len() == 20
        && stamp.ends_with('Z')
        && chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%dT%H-%M-%SZ").is_ok();
    ok.then_some((stamp, n))
}

/// The time a log's name holds.
pub fn run_time(name: &str) -> Option<DateTime<Utc>> {
    let (stamp, _) = run_key(name)?;
    chrono::NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%dT%H-%M-%SZ")
        .ok()
        .map(|t| t.and_utc())
}

/// The log's one-line summary: what follows [`RESULT`] on its last such line.
pub fn result_of(text: &str) -> Option<&str> {
    text.lines().rev().find_map(|l| l.strip_prefix(RESULT))
}
