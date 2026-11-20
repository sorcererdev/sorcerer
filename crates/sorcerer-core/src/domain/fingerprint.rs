//! Fingerprints: a short name for a unit's text that survives reformatting.
//!
//! A fingerprint is seven hex digits, such as `aee3e55`. Reindenting a function, or a formatter
//! spreading a call over several lines, keeps its fingerprint; editing what the code says
//! changes it, and so does editing a comment. Every verdict sorcerer records holds "for the unit
//! with this fingerprint", so a verdict lapses when the unit really changes and only then.

use std::fmt;

use crate::domain::fnv::fnv64_hex;
use crate::domain::unit::Span;

/// Seven lowercase hex digits naming a unit's text (see the module comment).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; 7]);

impl Fingerprint {
    /// Reads a fingerprint back from text. Only exactly seven lowercase hex digits are one:
    /// `"8fe4240"` is, `"8FE4240"` and `"8fe424"` are not (`None`).
    pub fn parse(s: &str) -> Option<Fingerprint> {
        let b = s.as_bytes();
        if b.len() != 7
            || !b
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
        {
            return None;
        }
        let mut out = [0u8; 7];
        out.copy_from_slice(b);
        Some(Fingerprint(out))
    }

    /// The seven digits as text.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("0000000")
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.as_str())
    }
}

impl serde::Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Fingerprint, D::Error> {
        let s = String::deserialize(d)?;
        Fingerprint::parse(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("not a fingerprint: {s}")))
    }
}

/// The fingerprint of some lines of a file, from `span.start` to `span.end`, counted from 1.
///
/// The lines are trimmed, blank lines are dropped, lines a formatter split are joined back
/// (see [`reflowed`]), and the result is hashed with FNV-1a; the fingerprint is the first seven
/// of the hash's sixteen hex digits.
///
/// Example: `f(a, b)` and the same call spread over four lines as `f(`, `a,`, `b,`, `)` both
/// have the fingerprint `8fe4240`.
pub fn fingerprint(text: &str, span: Span) -> Fingerprint {
    let lines = nonblank_trimmed_lines(text, span);
    let hex = fnv64_hex(reflowed(&lines).as_bytes());
    Fingerprint::parse(&hex[..7]).expect("hex digits")
}

/// The span's lines with their indentation and line endings trimmed, blank lines dropped.
fn nonblank_trimmed_lines(text: &str, span: Span) -> Vec<&str> {
    text.split('\n')
        .skip(span.start.saturating_sub(1) as usize)
        .take((span.end + 1).saturating_sub(span.start) as usize)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

/// Joins trimmed lines back the way they would read had a formatter not split them.
///
/// A break is a formatter's, and is undone, in four places: before a closing bracket (a
/// trailing comma just before it goes too), after an opening bracket, after a comma (one space
/// is kept), and before a leading comma. Every other break stays a newline. When any break was
/// undone, a space just inside a bracket is dropped too, since leading-comma style opens with
/// `[ a`.
///
/// Example: `["f(", "a,", "b,", ")"]` becomes `f(a, b)`; `["x = 1", "y = 2"]` stays two lines.
pub fn reflowed(lines: &[&str]) -> String {
    let Some((first, rest)) = lines.split_first() else {
        return String::new();
    };
    let (text, any_undone) =
        rest.iter()
            .fold(
                (first.to_string(), false),
                |(text, any_undone), next| match undo_formatter_break(&text, next) {
                    Some(joined) => (joined, true),
                    None => (format!("{text}\n{next}"), any_undone),
                },
            );
    if any_undone {
        without_spaces_inside_brackets(text)
    } else {
        text
    }
}

/// The text so far with the next line joined on, when the break between them is one a
/// formatter makes; `None` when it is a real line break.
fn undo_formatter_break(text: &str, next: &str) -> Option<String> {
    if next.starts_with([')', ']', '}']) {
        let without_trailing_comma = text.strip_suffix(',').unwrap_or(text);
        Some(format!("{without_trailing_comma}{next}"))
    } else if text.ends_with(['(', '[', '{']) {
        Some(format!("{text}{next}"))
    } else if text.ends_with(',') {
        Some(format!("{text} {next}"))
    } else if next.starts_with(',') {
        Some(format!("{text}{next}"))
    } else {
        None
    }
}

fn without_spaces_inside_brackets(text: String) -> String {
    [
        (" }", "}"),
        (" ]", "]"),
        (" )", ")"),
        ("{ ", "{"),
        ("[ ", "["),
        ("( ", "("),
    ]
    .iter()
    .fold(text, |text, (from, to)| text.replace(from, to))
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::unit::Span;

    fn fp(text: &str, start: u32, end: u32) -> Fingerprint {
        fingerprint(text, Span { start, end })
    }

    #[test]
    fn known_values() {
        assert_eq!(fp("def f():\n    return 1\n", 1, 2).to_string(), "aee3e55");
        assert_eq!(fp("f(a, b)\n", 1, 1).to_string(), "8fe4240");
    }

    #[test]
    fn a_reflow_keeps_it() {
        assert_eq!(fp("f(\n    a,\n    b,\n)\n", 1, 4), fp("f(a, b)\n", 1, 1));
    }

    #[test]
    fn crlf_and_indentation_and_blank_lines_keep_it() {
        assert_eq!(
            fp("def f():\r\n\r\n        return 1\r\n", 1, 3),
            fp("def f():\n    return 1\n", 1, 2)
        );
    }

    #[test]
    fn a_real_edit_changes_it() {
        assert_ne!(fp("def f():\n    return 2\n", 1, 2).to_string(), "aee3e55");
    }

    #[test]
    fn a_comment_change_changes_it() {
        assert_ne!(fp("x = 1  # a\n", 1, 1), fp("x = 1  # b\n", 1, 1));
    }

    #[test]
    fn only_the_span_counts() {
        assert_eq!(fp("a\nb\nc\n", 2, 2), fp("z\nb\ny\n", 2, 2));
    }

    #[test]
    fn it_round_trips_as_a_string() {
        let f = fp("f(a, b)\n", 1, 1);
        assert_eq!(serde_json::to_string(&f).unwrap(), "\"8fe4240\"");
        assert_eq!(
            serde_json::from_str::<Fingerprint>("\"8fe4240\"").unwrap(),
            f
        );
        assert_eq!(Fingerprint::parse("8fe4240"), Some(f));
        assert_eq!(Fingerprint::parse("8FE4240"), None);
        assert_eq!(Fingerprint::parse("8fe424"), None);
    }
}
