//! How findings and tables are printed.

use serde_json::json;
use sorcerer_core::domain::unit::Span;

use crate::domain::finding::Finding;

/// Columns padded to their widest cell, two spaces apart, no trailing space.
pub(super) fn table(header: &[&str], rows: &[Vec<String>]) -> String {
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

/// S, cut to N characters (the last an ellipsis) when it is longer.
pub(super) fn cut_to(n: usize, s: &str) -> String {
    if s.chars().count() > n {
        let mut t: String = s.chars().take(n - 1).collect();
        t.push('…');
        t
    } else {
        s.to_string()
    }
}

/// The cited lines whole, two either side, numbered as the claim prompt
/// numbers them; a range past the end stops at the last line.
pub(super) fn cited_range(text: &str, lines: Span) -> String {
    let ls: Vec<&str> = text.lines().collect();
    let lo = lines.start.saturating_sub(2).max(1) as usize;
    let hi = (lines.end.max(lines.start) as usize + 2).min(ls.len());
    (lo..=hi)
        .filter_map(|n| ls.get(n - 1).map(|l| format!("{n:>4} {l}")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `2` for one line, `2-5` for a range.
pub(super) fn lines_text(a: u32, z: u32) -> String {
    if a == z {
        a.to_string()
    } else {
        format!("{a}-{z}")
    }
}

/// `file:line  issue type  statement  (confidence)`, the statement wrapped to WIDTH
/// under itself, then the evidence and the `scry why` line.
pub(super) fn finding_text(f: &Finding, width: usize) -> String {
    let head = format!(
        "{}:{}  {}  ",
        f.place.file.display(),
        f.place.lines.start,
        f.issue_type
    );
    let indent = head.chars().count();
    let room = width.saturating_sub(indent).max(30);
    let tag = format!("({:.2})", f.scores.confidence.unwrap_or(0.0));
    let lines = wrapped_with_tag(&f.statement, &tag, room);
    let mut text = head;
    text.push_str(&lines.join(&format!("\n{}", " ".repeat(indent))));
    text.push('\n');
    if !f.evidence.is_empty() {
        text.push_str(&evidence_line(f));
    }
    text.push_str(&format!("  scry why {}\n", f.id.0));
    text
}

/// STATEMENT's words in lines of at most ROOM characters, TAG two spaces
/// after the last word, or on a line of its own when it does not fit.
fn wrapped_with_tag(statement: &str, tag: &str, room: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![String::new()];
    for word in statement.split_whitespace() {
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
        last.push_str(tag);
    } else {
        lines.push(tag.to_string());
    }
    lines
}

/// `  evidence: orders.py:2, orders.py:5-6`.
fn evidence_line(f: &Finding) -> String {
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
    format!("  evidence: {}\n", cited.join(", "))
}

pub(super) fn finding_json(f: &Finding) -> serde_json::Value {
    json!({
        "id": f.id.0,
        "issue_type": f.issue_type,
        "rule": f.rule,
        "statement": f.statement,
        "file": f.place.file,
        "lines": [f.place.lines.start, f.place.lines.end],
        "unit": f.place.unit,
        "evidence": f.evidence.iter().map(|e| json!({
            "file": e.file,
            "lines": [e.lines.start, e.lines.end],
        })).collect::<Vec<_>>(),
        "confidence": f.scores.confidence,
        "verdict": f.verdict.as_ref().map(|v| v.verdict.clone()),
        "status": f.status,
    })
}
