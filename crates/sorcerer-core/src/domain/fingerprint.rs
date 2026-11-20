//! Fingerprints: seven hex digits naming a unit's text with formatting
//! removed, so a formatter's reflow keeps it and a real edit changes it.
//! Every verdict sorcerer records is "true when the unit hashed to this".
//!
//! The function: the span's lines trimmed, blank lines dropped, joined
//! by newlines except where a formatter would have split one line (see
//! [`reflowed`]), FNV-1a 64 over the UTF-8 bytes, the first seven of its
//! sixteen hex digits. Comments are kept: changing one changes it.

use std::fmt;

use crate::domain::fnv::fnv64_hex;
use crate::domain::unit::Span;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; 7]);

impl Fingerprint {
    /// Seven lowercase hex digits, or None.
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

/// The fingerprint of SPAN's lines (1-based, inclusive) of TEXT.
pub fn fingerprint(text: &str, span: Span) -> Fingerprint {
    let lines: Vec<&str> = text
        .split('\n')
        .skip(span.start.saturating_sub(1) as usize)
        .take((span.end + 1).saturating_sub(span.start) as usize)
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let hex = fnv64_hex(reflowed(&lines).as_bytes());
    Fingerprint::parse(&hex[..7]).expect("hex digits")
}

/// Trimmed lines joined as one would be had a formatter not split it:
/// after an opening bracket and before a closing one with nothing between
/// (a trailing comma dropped), after a comma with one space, before a
/// leading comma with none; every other break is a newline. Where any break
/// was joined, a space just inside a bracket goes too (leading-comma style
/// opens `[ a`).
pub fn reflowed(lines: &[&str]) -> String {
    let Some((first, rest)) = lines.split_first() else {
        return String::new();
    };
    let mut acc = first.to_string();
    let mut joined = false;
    for n in rest {
        if n.starts_with([')', ']', '}']) {
            joined = true;
            if acc.ends_with(',') {
                acc.pop();
            }
            acc.push_str(n);
        } else if acc.ends_with(['(', '[', '{']) {
            joined = true;
            acc.push_str(n);
        } else if acc.ends_with(',') {
            joined = true;
            acc.push(' ');
            acc.push_str(n);
        } else if n.starts_with(',') {
            joined = true;
            acc.push_str(n);
        } else {
            acc.push('\n');
            acc.push_str(n);
        }
    }
    if joined {
        for (from, to) in [
            (" }", "}"),
            (" ]", "]"),
            (" )", ")"),
            ("{ ", "{"),
            ("[ ", "["),
            ("( ", "("),
        ] {
            acc = acc.replace(from, to);
        }
    }
    acc
}

#[cfg(test)]
#[allow(clippy::too_many_lines)]
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
