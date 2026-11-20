//! Pieces of the run log's text (headings, fenced blocks, dollars, counts,
//! a citation's outcome, the scope's section), shared by the parts of scry
//! that write the log. Only the words: where the log goes is the run log
//! port's business.

use crate::domain::finding::StoredCitation;
use crate::domain::scope::{Base, Scope, ScopeRequest, ScopedFile};

/// A level-two Markdown heading with a blank line either side:
/// `## Scope`.
pub fn section(title: &str) -> String {
    format!("\n## {title}\n\n")
}

/// A level-three Markdown heading with a blank line either side:
/// `### orders.py`.
pub fn sub(title: &str) -> String {
    format!("\n### {title}\n\n")
}

/// The text, unchanged, in a Markdown code block. The fence is at least
/// three backticks, and one longer than the longest run of backticks in
/// the text, so a reply that holds its own code block cannot close ours
/// early.
///
/// Example: text holding a ```` ``` ```` block is fenced with four
/// backticks.
pub fn block(text: &str) -> String {
    let fence = "`".repeat((longest_backtick_run(text) + 1).max(3));
    let line_end = if text.ends_with('\n') { "" } else { "\n" };
    format!("{fence}text\n{text}{line_end}{fence}\n\n")
}

fn longest_backtick_run(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

/// Dollars to the tenth of a cent: `$0.042`. A negative zero shows as
/// `$0.000`, not `$-0.000`.
pub fn usd(dollars: f64) -> String {
    format!("${:.3}", if dollars == 0.0 { 0.0 } else { dollars })
}

/// A count and its noun, plural unless it is 1: `1 unit`, `2 units`,
/// `0 units`.
pub fn count(n: usize, what: &str) -> String {
    format!("{n} {what}{}", if n == 1 { "" } else { "s" })
}

/// A size: in bytes under 1 KB (`312 B`), else in KB to one decimal
/// (`4.1 KB`).
pub fn bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else {
        format!("{:.1} KB", n as f64 / 1024.0)
    }
}

/// A citation and whether it was found, with the parts it has.
///
/// Example: `` `orders.py` `Order.cancel` line 41: confirmed ``; the
/// outcome may also be `not found` or `unit not found, confirmed on line
/// 41`.
pub fn citation(cited: &StoredCitation) -> String {
    let unit = cited.unit.as_ref().map(|unit| format!(" `{unit}`"));
    let line = cited.line.map(|line| format!(" line {line}"));
    format!(
        "`{}`{}{}: {}",
        cited.path,
        unit.unwrap_or_default(),
        line.unwrap_or_default(),
        cited.outcome()
    )
}

/// The scope's section of the run log: why the pass runs, the revision it
/// compares against, each file in scope with the units it added or
/// changed, and each file left out with why.
///
/// Example:
/// ```text
/// ## Scope
///
/// - scope: the working tree
/// - base: `HEAD`
/// - `orders.py`: in; added or changed: `Order.cancel` (10-20)
/// - `notes.txt`: skipped (no language covers it)
/// ```
pub fn scope_text(scope: &Scope) -> String {
    let lines = [
        format!("- scope: {}", why_the_pass_runs(&scope.request)),
        format!("- base: {}", base_text(&scope.base)),
    ]
    .into_iter()
    .chain(
        scope
            .files
            .iter()
            .map(|file| file_in_scope_line(scope, file)),
    )
    .chain(
        scope
            .skipped
            .iter()
            .map(|(file, why)| format!("- `{}`: skipped ({why})", file.display())),
    );
    let body: String = lines.map(|line| line + "\n").collect();
    section("Scope") + &body
}

fn why_the_pass_runs(request: &ScopeRequest) -> String {
    match request {
        ScopeRequest::Files(paths) => {
            let named: Vec<String> = paths
                .iter()
                .map(|path| format!("`{}`", path.display()))
                .collect();
            format!("the files named: {}", named.join(", "))
        }
        ScopeRequest::Since(rev) => format!("the files changed since `{rev}`"),
        ScopeRequest::Working => "the working tree".into(),
    }
}

fn base_text(base: &Base) -> String {
    match base {
        Base::NoCommitYet => "none (no commit yet: every file is new)".to_string(),
        Base::Revision(rev) => format!("`{rev}`"),
    }
}

/// A file in scope: deleted, or the units it added or changed with their
/// lines.
fn file_in_scope_line(scope: &Scope, file: &ScopedFile) -> String {
    let name = file.file.display();
    if file.deleted {
        return format!("- `{name}`: in, deleted (its findings are gone)");
    }
    let units: Vec<String> = scope
        .units
        .iter()
        .filter(|scoped| scoped.file == file.file)
        .map(|scoped| {
            let span = scoped.unit.span;
            format!("`{}` ({}-{})", scoped.unit.name, span.start, span.end)
        })
        .collect();
    if units.is_empty() {
        format!("- `{name}`: in; no unit added or changed")
    } else {
        format!("- `{name}`: in; added or changed: {}", units.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::scope::{ScopedFile, ScopedUnit};
    use sorcerer_core::domain::fingerprint::Fingerprint;
    use sorcerer_core::domain::unit::{Span, Unit, UnitKind};

    fn file(name: &str, deleted: bool) -> ScopedFile {
        ScopedFile {
            file: name.into(),
            diff: String::new(),
            units_now: vec![],
            deleted,
        }
    }

    fn unit(file: &str, name: &str, start: u32, end: u32) -> ScopedUnit {
        ScopedUnit {
            file: file.into(),
            unit: Unit {
                name: name.into(),
                kind: UnitKind::Function,
                span: Span { start, end },
                body: None,
                fingerprint: Fingerprint::parse("0123abc").unwrap(),
            },
            header: None,
        }
    }

    #[test]
    fn the_scope_section_names_each_file_and_why() {
        let scope = Scope {
            request: ScopeRequest::Files(vec!["orders.txt".into(), "old.txt".into()]),
            base: Base::Revision("HEAD".into()),
            units: vec![
                unit("orders.txt", "Order.cancel", 10, 20),
                unit("orders.txt", "refund", 22, 30),
            ],
            files: vec![
                file("orders.txt", false),
                file("tax.txt", false),
                file("old.txt", true),
            ],
            skipped: vec![("notes.md".into(), "no language".into())],
        };
        assert_eq!(
            scope_text(&scope),
            "\n## Scope\n\n\
             - scope: the files named: `orders.txt`, `old.txt`\n\
             - base: `HEAD`\n\
             - `orders.txt`: in; added or changed: `Order.cancel` (10-20), `refund` (22-30)\n\
             - `tax.txt`: in; no unit added or changed\n\
             - `old.txt`: in, deleted (its findings are gone)\n\
             - `notes.md`: skipped (no language)\n"
        );
        let first_commit = Scope {
            request: ScopeRequest::Working,
            base: Base::NoCommitYet,
            units: vec![],
            files: vec![],
            skipped: vec![],
        };
        assert_eq!(
            scope_text(&first_commit),
            "\n## Scope\n\n- scope: the working tree\n- base: none (no commit yet: every file is new)\n"
        );
    }

    #[test]
    fn a_block_fence_outruns_the_backticks_inside() {
        assert_eq!(block("x"), "```text\nx\n```\n\n");
        assert_eq!(block("a ```` b\n"), "`````text\na ```` b\n`````\n\n");
    }
}
