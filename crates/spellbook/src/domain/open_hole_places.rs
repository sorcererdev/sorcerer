//! Whether a hole is still open: its placeholder still in the file, and on which line now.

use crate::domain::record::OpenHole;

/// How far from its recorded line a short placeholder is looked for, in lines.
pub const NEARBY_LINES: u32 = 20;

/// The line (from 1) where the hole's placeholder is now, or `None` when it is gone. A
/// placeholder that says `hole <name>:` is found by that text anywhere in the file, nearest the
/// recorded line first; a short one (`todo_field`) is looked for as a whole word on the recorded
/// line, then on the lines around it, at most [`NEARBY_LINES`] away.
///
/// Example: `todo_field` recorded at line 10 and now on line 12 (two lines were added above) is
/// open at 12; once someone writes `id: str` there, it is gone.
pub fn still_open(hole: &OpenHole, text: &str) -> Option<u32> {
    let lines: Vec<&str> = text.lines().collect();
    let marker = format!("hole {}:", hole.name);
    let marked = hole.placeholder.contains(&marker);
    let (needle, reach) = if marked {
        (
            marker.as_str(),
            u32::try_from(lines.len()).unwrap_or(u32::MAX),
        )
    } else {
        (hole.placeholder.as_str(), NEARBY_LINES)
    };
    // A short placeholder also has to stand between the same signs as when it was written, so
    // `()` in `Result<(), E>` is not the `()` of `pub id: (),`.
    let edges = if marked { "" } else { hole.edges.as_str() };
    if needle.trim().is_empty() {
        return None;
    }
    lines_nearest(hole.line, reach).find(|line| {
        lines
            .get((*line - 1) as usize)
            .is_some_and(|row| holds_whole(row, needle, edges))
    })
}

/// The recorded line, then the lines one away, two away… up to `reach` away (lines from 1).
fn lines_nearest(line: u32, reach: u32) -> impl Iterator<Item = u32> {
    std::iter::once(line).chain((1..=reach).flat_map(move |distance| {
        [line.checked_sub(distance), line.checked_add(distance)]
            .into_iter()
            .flatten()
            .filter(|candidate| *candidate >= 1)
    }))
}

/// Whether the row holds the needle not glued to a longer word: `todo_field` is not in
/// `todo_fields`.
fn holds_whole(row: &str, needle: &str, edges: &str) -> bool {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    row.match_indices(needle).any(|(at, _)| {
        let before = row[..at].chars().next_back();
        let after = row[at + needle.len()..].chars().next();
        let starts_clean = !needle.starts_with(is_word) || !before.is_some_and(is_word);
        let ends_clean = !needle.ends_with(is_word) || !after.is_some_and(is_word);
        starts_clean && ends_clean && between_the_edges(row, at, needle.len(), edges)
    })
}

/// Whether the signs beside the needle at `at` are the recorded `edges` (always, when none).
fn between_the_edges(row: &str, at: usize, len: usize, edges: &str) -> bool {
    let mut wanted = edges.chars();
    let (Some(before), Some(after)) = (wanted.next(), wanted.next()) else {
        return true;
    };
    let seen_before = row[..at].chars().rev().find(|c| !c.is_whitespace());
    let seen_after = row[at + len..].chars().find(|c| !c.is_whitespace());
    seen_before.unwrap_or('\n') == before && seen_after.unwrap_or('\n') == after
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hole(name: &str, placeholder: &str, line: u32) -> OpenHole {
        OpenHole {
            generation: "g".into(),
            generator: "command".into(),
            file: "a.x".into(),
            line,
            name: name.into(),
            kind: "ident".into(),
            instruction: "x".into(),
            placeholder: placeholder.into(),
            edges: String::new(),
        }
    }

    #[test]
    fn a_short_placeholder_is_found_nearby_as_a_whole_word() {
        let text = "a\nb\nc\n    todo_field: object\n";
        assert_eq!(still_open(&hole("field", "todo_field", 2), text), Some(4));
        assert_eq!(still_open(&hole("field", "todo_fiel", 2), text), None);
        let far = format!("{}todo_field\n", "x\n".repeat(30));
        assert_eq!(still_open(&hole("field", "todo_field", 1), &far), None);
    }

    #[test]
    fn a_marked_placeholder_is_found_anywhere() {
        let text = format!("{}fail(\"hole handle: x\")\n", "x\n".repeat(40));
        assert_eq!(
            still_open(&hole("handle", "fail(\"hole handle: x\")", 1), &text),
            Some(41)
        );
        assert_eq!(
            still_open(&hole("handle", "fail(\"hole handle: x\")", 1), "done\n"),
            None
        );
    }

    #[test]
    fn a_type_placeholder_is_not_the_same_signs_elsewhere() {
        let mut open = hole("type", "()", 2);
        open.edges = ":,".into();
        let filled = "struct A {\n    pub id: String,\n}\nfn f() -> Result<(), E> {\n}\n";
        assert_eq!(still_open(&open, filled), None);
        let waiting = "struct A {\n    pub id: (),\n}\nfn f() -> Result<(), E> {\n}\n";
        assert_eq!(still_open(&open, waiting), Some(2));
    }

    #[test]
    fn a_placeholder_of_signs_only_matches_as_text() {
        assert_eq!(still_open(&hole("type", "()", 1), "x: ()\n"), Some(1));
    }
}
