//! A region: one candidate for Jev, cut from what the person changed. It lives on its text,
//! not its line: a region is found again wherever its lines now are, and is gone once they
//! change.

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::fnv::fnv64_hex;
use sorcerer_core::domain::unit::Span;

/// What a region was cut from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RegionPlace {
    /// A comment the language's queries capture.
    Comment,
    /// A paragraph: of a file no language covers, or what its language captures as prose.
    Prose,
    /// A unit whose body does nothing yet.
    Stub,
    /// Changed code with none of the above.
    Hunk,
}

/// One region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// The file and the text, hashed: the same text in the same file is the same region.
    pub id: String,
    /// Relative to the root, with `/`.
    pub file: String,
    /// Where it was when it was cut, from 1.
    pub lines: Span,
    pub text: String,
    pub place: RegionPlace,
    /// The unit holding it, by name, when there is one.
    pub unit: Option<String>,
}

impl Region {
    pub fn new(
        file: &str,
        lines: Span,
        text: &str,
        place: RegionPlace,
        unit: Option<String>,
    ) -> Region {
        Region {
            id: region_id(file, text),
            file: file.to_string(),
            lines,
            text: text.to_string(),
            place,
            unit,
        }
    }

    /// Where the region's lines are in the text now, from 1: the first run of lines equal to
    /// its own (spaces at their ends aside), nearest its old place; `None` once they changed.
    ///
    /// Example: a two-line comment cut at lines 4-5 is at 6-7 after two lines were added above
    /// it, and nowhere once a word of it changed.
    pub fn found_in(&self, text: &str) -> Option<Span> {
        let wanted: Vec<&str> = self.text.lines().map(str::trim_end).collect();
        let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
        if wanted.is_empty() || wanted.len() > lines.len() {
            return None;
        }
        (0..=lines.len() - wanted.len())
            .filter(|start| lines[*start..*start + wanted.len()] == wanted[..])
            .min_by_key(|start| (*start as i64 + 1 - self.lines.start as i64).abs())
            .map(|start| Span {
                start: start as u32 + 1,
                end: (start + wanted.len()) as u32,
            })
    }
}

/// A region's id: `r` and the file and text hashed (the text's spacing squashed, so
/// re-indenting a comment keeps its id).
pub fn region_id(file: &str, text: &str) -> String {
    let squashed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "r{}",
        &fnv64_hex(format!("{file}\n{squashed}").as_bytes())[..12]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_is_found_where_its_lines_moved_and_lost_when_they_changed() {
        let region = Region::new(
            "a.txt",
            Span { start: 2, end: 3 },
            "# one\n# two\n",
            RegionPlace::Comment,
            None,
        );
        assert_eq!(
            region.found_in("x\n# one\n# two\n"),
            Some(Span { start: 2, end: 3 })
        );
        assert_eq!(
            region.found_in("x\ny\nz\n# one\n# two\n"),
            Some(Span { start: 4, end: 5 })
        );
        assert_eq!(region.found_in("x\n# one\n# three\n"), None);
    }

    #[test]
    fn spacing_keeps_the_id_and_a_word_changes_it() {
        assert_eq!(region_id("a", "# one  two"), region_id("a", "  # one two"));
        assert_ne!(region_id("a", "# one"), region_id("a", "# two"));
        assert_ne!(region_id("a", "# one"), region_id("b", "# one"));
    }
}
