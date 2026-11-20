//! The run log's text: headings, fenced blocks, dollars, counts, a
//! citation's outcome and the scope's section. Pure formatting; where the
//! log goes is the run log port's business.

use crate::domain::finding::StoredCitation;
use crate::domain::scope::{Base, Scope, ScopeRequest};

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

/// `312 B`, `4.1 KB`.
pub fn bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}

/// `` `path` `unit` line N: confirmed `` (or `not found`, or `unit not found,
/// confirmed on line N`).
pub fn citation(c: &StoredCitation) -> String {
    let mut s = format!("`{}`", c.path);
    if let Some(u) = &c.unit {
        s.push_str(&format!(" `{u}`"));
    }
    if let Some(l) = c.line {
        s.push_str(&format!(" line {l}"));
    }
    format!("{s}: {}", c.outcome())
}

/// The scope's section: why the pass runs, the base, each file in scope
/// with its units, and each file left out with why.
pub fn scope_text(scope: &Scope) -> String {
    let mut s = section("Scope");
    let why = match &scope.request {
        ScopeRequest::Files(ps) => format!(
            "the files named: {}",
            ps.iter()
                .map(|p| format!("`{}`", p.display()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ScopeRequest::Since(rev) => format!("the files changed since `{rev}`"),
        ScopeRequest::Working => "the working tree".into(),
    };
    s.push_str(&format!("- scope: {why}\n"));
    let base = match &scope.base {
        Base::NoCommitYet => "none (no commit yet: every file is new)".to_string(),
        Base::Revision(rev) => format!("`{rev}`"),
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
