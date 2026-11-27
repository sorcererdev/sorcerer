//! Cutting regions from what the person changed: the changed lines of a file, laid over its
//! shape (its comments, its units, whether a language covers it), become the regions Jev is
//! asked about. No language is named: comments are what the language's queries capture.

use std::collections::HashSet;

use similar::{DiffTag, TextDiff};
use sorcerer_core::domain::unit::Span;

use crate::domain::region::{Region, RegionPlace};

/// A hunk of more lines than this is not a region: too much for one question.
pub const HUNK_LINES: u32 = 40;

/// A file's shape, from its grammar: its comments' lines, its units (name, lines, and whether
/// the body does nothing yet), and whether a language covers it at all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shape {
    pub comments: Vec<Span>,
    pub units: Vec<ShapeUnit>,
    pub has_language: bool,
}

/// One unit of a file's shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeUnit {
    pub name: String,
    pub lines: Span,
    pub is_stub: bool,
}

/// The lines of `after` (from 1) the person changed since `before`, as runs. A deletion counts
/// as the line after it, unless every deleted line is still in the file (it moved). A change is
/// left out when it is not the person's typing:
///
/// - a line a tool landed in the file a moment ago (its trimmed text is in `landed_by_tools`:
///   spellbook's sync, a conjure step, an apprentice task);
/// - a line whose trimmed text was already in the file (moved, as a formatter sorting imports
///   does, or re-indented);
/// - a change that is the old lines reflowed: the same text once whitespace is taken out (a
///   formatter rewrapping a call, or adding and removing blank lines);
/// - a blank line.
///
/// Example: from `a\nb\nc\n` to `a\nB\nc\nd\n`, lines 2 and 4 changed; with `d` landed by a
/// tool, only line 2.
pub fn persons_changed_lines(
    before: &str,
    after: &str,
    landed_by_tools: &HashSet<String>,
) -> Vec<Span> {
    let after_lines: Vec<&str> = after.lines().collect();
    let before_lines: Vec<&str> = before.lines().collect();
    let already_there: HashSet<&str> = before_lines.iter().map(|line| line.trim()).collect();
    let still_there: HashSet<&str> = after_lines.iter().map(|line| line.trim()).collect();
    let persons = |line: &str| {
        let line = line.trim();
        !line.is_empty() && !already_there.contains(line) && !landed_by_tools.contains(line)
    };
    let after_count = after_lines.len().max(1) as u32;
    let lines: Vec<u32> = TextDiff::from_lines(before, after)
        .ops()
        .iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .filter(|op| {
            let old =
                &before_lines[op.old_range().start..op.old_range().end.min(before_lines.len())];
            let new = &after_lines[op.new_range().start..op.new_range().end.min(after_lines.len())];
            without_whitespace(old) != without_whitespace(new)
        })
        .flat_map(|op| {
            let range = op.new_range();
            if range.is_empty() {
                let deleted = &before_lines[op.old_range()];
                let moved = deleted.iter().all(|line| still_there.contains(line.trim()));
                return if moved {
                    Vec::new()
                } else {
                    vec![(range.start as u32 + 1).min(after_count)]
                };
            }
            range
                .filter(|&index| after_lines.get(index).is_some_and(|line| persons(line)))
                .map(|index| index as u32 + 1)
                .collect()
        })
        .collect();
    runs_of(&lines)
}

fn without_whitespace(lines: &[&str]) -> String {
    lines
        .iter()
        .flat_map(|line| line.chars())
        .filter(|c| !c.is_whitespace())
        .collect()
}

/// Line numbers, in order, joined into runs of neighbouring lines.
fn runs_of(lines: &[u32]) -> Vec<Span> {
    lines.iter().fold(Vec::new(), |mut runs: Vec<Span>, &line| {
        match runs.last_mut() {
            Some(last) if line <= last.end + 1 => last.end = last.end.max(line),
            _ => runs.push(Span {
                start: line,
                end: line,
            }),
        }
        runs
    })
}

/// The regions the changed lines of a file make, in file order, each once:
///
/// 1. a comment block (comments on neighbouring lines, joined) touching a changed line;
/// 2. in a file no language covers, a paragraph (lines between blank lines) touching one;
/// 3. a unit whose body does nothing yet, when its lines changed;
/// 4. the rest of a changed run, outside comments, when it holds a letter and is at most 40
///    lines.
///
/// Example: in a file of two functions, typing a comment above the second and a line in its
/// body gives a comment region and a hunk region, both naming the second function.
pub fn regions_of(file: &str, text: &str, changed: &[Span], shape: &Shape) -> Vec<Region> {
    let lines: Vec<&str> = text.lines().collect();
    let line_text = |span: Span| -> String {
        lines[(span.start - 1) as usize..span.end.min(lines.len() as u32) as usize]
            .iter()
            .map(|line| format!("{line}\n"))
            .collect()
    };
    let unit_of = |span: Span| innermost_unit(shape, span).map(|unit| unit.name.clone());
    let mut regions: Vec<Region> = Vec::new();
    let mut add = |span: Span, place: RegionPlace| {
        let region = Region::new(file, span, &line_text(span), place, unit_of(span));
        if !regions.iter().any(|known| known.id == region.id) {
            regions.push(region);
        }
    };
    let blocks = comment_blocks(&shape.comments);
    for run in changed
        .iter()
        .filter(|run| run.start as usize <= lines.len())
    {
        for block in blocks.iter().filter(|block| overlaps(block, run)) {
            add(*block, RegionPlace::Comment);
        }
        if !shape.has_language {
            for paragraph in paragraphs_touching(&lines, *run) {
                add(paragraph, RegionPlace::Prose);
            }
            continue;
        }
        let stubs = shape
            .units
            .iter()
            .filter(|unit| unit.is_stub && overlaps(&unit.lines, run));
        for stub in stubs {
            add(stub.lines, RegionPlace::Stub);
        }
        for hunk in outside_comments(*run, &blocks) {
            let span_text = line_text(hunk);
            let in_a_stub = shape
                .units
                .iter()
                .any(|unit| unit.is_stub && unit.lines.contains(&hunk));
            if !in_a_stub
                && hunk.end - hunk.start < HUNK_LINES
                && span_text.chars().any(char::is_alphabetic)
            {
                add(hunk, RegionPlace::Hunk);
            }
        }
    }
    regions.sort_by_key(|region| region.lines.start);
    regions
}

/// Comments on neighbouring lines, joined into blocks.
fn comment_blocks(comments: &[Span]) -> Vec<Span> {
    let mut sorted = comments.to_vec();
    sorted.sort();
    sorted
        .into_iter()
        .fold(Vec::new(), |mut blocks: Vec<Span>, comment| {
            match blocks.last_mut() {
                Some(last) if comment.start <= last.end + 1 => last.end = last.end.max(comment.end),
                _ => blocks.push(comment),
            }
            blocks
        })
}

fn overlaps(a: &Span, b: &Span) -> bool {
    a.start <= b.end && b.start <= a.end
}

/// The parts of a run that no comment block covers.
fn outside_comments(run: Span, blocks: &[Span]) -> Vec<Span> {
    let covered = |line: u32| blocks.iter().any(|block| block.holds(line));
    (run.start..=run.end).fold(Vec::new(), |mut parts: Vec<Span>, line| {
        if covered(line) {
            return parts;
        }
        match parts.last_mut() {
            Some(last) if last.end + 1 == line => last.end = line,
            _ => parts.push(Span {
                start: line,
                end: line,
            }),
        }
        parts
    })
}

/// The paragraphs (runs of lines that are not blank) touching a run.
fn paragraphs_touching(lines: &[&str], run: Span) -> Vec<Span> {
    let blank = |line: u32| {
        lines
            .get(line as usize - 1)
            .is_none_or(|text| text.trim().is_empty())
    };
    let mut found = Vec::new();
    let mut line = run.start;
    while line <= run.end && line as usize <= lines.len() {
        if blank(line) {
            line += 1;
            continue;
        }
        let mut start = line;
        while start > 1 && !blank(start - 1) {
            start -= 1;
        }
        let mut end = line;
        while (end as usize) < lines.len() && !blank(end + 1) {
            end += 1;
        }
        found.push(Span { start, end });
        line = end + 1;
    }
    found
}

/// The smallest unit holding the span.
fn innermost_unit(shape: &Shape, span: Span) -> Option<&ShapeUnit> {
    shape
        .units
        .iter()
        .filter(|unit| unit.lines.contains(&span))
        .min_by_key(|unit| unit.lines.end - unit.lines.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: u32, end: u32) -> Span {
        Span { start, end }
    }

    fn unit(name: &str, lines: Span, is_stub: bool) -> ShapeUnit {
        ShapeUnit {
            name: name.into(),
            lines,
            is_stub,
        }
    }

    #[test]
    fn changed_lines_are_runs_in_the_new_text() {
        let none = HashSet::new();
        assert_eq!(
            persons_changed_lines("a\nb\nc\n", "a\nB\nc\nd\n", &none),
            [span(2, 2), span(4, 4)]
        );
        assert_eq!(
            persons_changed_lines("a\nb\nc\n", "a\nc\n", &none),
            [span(2, 2)]
        );
        assert!(persons_changed_lines("a\n", "a\n", &none).is_empty());
    }

    #[test]
    fn what_tools_landed_moved_or_reflowed_is_not_the_persons() {
        let landed = HashSet::from(["pub mod clock;".to_string()]);
        // a tool's line beside the person's
        assert_eq!(
            persons_changed_lines(
                "pub mod a;\n",
                "pub mod a;\npub mod clock;\n# why?\n",
                &landed
            ),
            [span(3, 3)]
        );
        // imports sorted: the lines were there
        assert!(
            persons_changed_lines(
                "import b\nimport a\nx = 1\n",
                "import a\nimport b\nx = 1\n",
                &landed
            )
            .is_empty()
        );
        // a call rewrapped, and a blank line added
        let one = "f(alpha, beta)\n";
        let wrapped = "f(\n    alpha,\n    beta\n)\n\n";
        assert!(persons_changed_lines(one, wrapped, &landed).is_empty());
    }

    #[test]
    fn a_changed_comment_is_its_whole_block_and_code_beside_it_a_hunk() {
        let text = "f():\n  // why is this\n  // slow?\n  call()\n";
        let shape = Shape {
            comments: vec![span(2, 2), span(3, 3)],
            units: vec![unit("f", span(1, 4), false)],
            has_language: true,
        };
        let regions = regions_of("a.txt", text, &[span(3, 4)], &shape);
        assert_eq!(regions.len(), 2);
        assert_eq!(
            (regions[0].place, regions[0].lines),
            (RegionPlace::Comment, span(2, 3))
        );
        assert_eq!(regions[0].text, "  // why is this\n  // slow?\n");
        assert_eq!(regions[0].unit.as_deref(), Some("f"));
        assert_eq!(
            (regions[1].place, regions[1].lines),
            (RegionPlace::Hunk, span(4, 4))
        );
    }

    #[test]
    fn a_stub_is_one_region_and_its_body_no_hunk() {
        let text = "g():\n  nothing\n";
        let shape = Shape {
            comments: Vec::new(),
            units: vec![unit("g", span(1, 2), true)],
            has_language: true,
        };
        let regions = regions_of("a.txt", text, &[span(1, 2)], &shape);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].place, RegionPlace::Stub);
    }

    #[test]
    fn prose_is_cut_by_paragraph() {
        let text = "One idea.\nStill it.\n\nAnother.\n";
        let regions = regions_of("notes.txt", text, &[span(2, 2)], &Shape::default());
        assert_eq!(regions.len(), 1);
        assert_eq!(
            (regions[0].place, regions[0].lines),
            (RegionPlace::Prose, span(1, 2))
        );
    }

    #[test]
    fn a_hunk_with_no_letter_or_too_long_is_nothing() {
        let shape = Shape {
            has_language: true,
            ..Shape::default()
        };
        assert!(regions_of("a.txt", "}\n)\n", &[span(1, 2)], &shape).is_empty());
        let long = "x\n".repeat(50);
        assert!(regions_of("a.txt", &long, &[span(1, 50)], &shape).is_empty());
    }
}
