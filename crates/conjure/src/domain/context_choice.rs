//! Choosing the related files a step is shown: whole files, the closest first, each kept while
//! it fits the step's budget and left out whole when it does not. No file is ever cut.

use crate::domain::situation::RelatedFile;

/// The related files chosen for one step may come to about 6,000 tokens in all (some 24,000
/// characters, or 600 lines of code).
pub const CONTEXT_BUDGET_TOKENS: usize = 6_000;

/// Roughly how many tokens a model reads for a text: one for every 4 bytes, rounded up.
///
/// Example: a 1,000-character file is about 250 tokens.
pub fn tokens_of(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// How close a related file is to the step, closest first: a closer file is kept before a
/// farther one when they do not all fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Closeness {
    /// A file the cursor's file imports, and whose names the unit under the cursor uses.
    UsedByTheUnit,
    /// A file the cursor's file imports.
    Imported,
    /// A file beside an import, in its folder, using its names (a port's adapter).
    BesideAnImport,
    /// The file one folder up from an import that declares its folder (`ports/mod.rs`).
    DeclaringAnImport,
}

/// The related files a step is shown, closest first, and those left out whole for room.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chosen {
    pub kept: Vec<RelatedFile>,
    pub left_for_room: Vec<RelatedFile>,
}

/// The files that fit `budget` tokens, taken in the order given (closest first): each file is
/// kept whole when it fits in what is left, and left out whole when it does not, so a smaller,
/// farther file may still come in after a large one is left out. No file is ever cut.
///
/// Example: with a budget of 100 tokens, files of 60, 50 and 30 tokens keep the first and the
/// third (90 tokens) and leave the second out.
pub fn chosen_within(files: Vec<RelatedFile>, budget: usize) -> Chosen {
    files
        .into_iter()
        .fold((Chosen::default(), budget), |(mut chosen, left), file| {
            let tokens = tokens_of(&file.text);
            if tokens <= left {
                chosen.kept.push(file);
                (chosen, left - tokens)
            } else {
                chosen.left_for_room.push(file);
                (chosen, left)
            }
        })
        .0
}

/// The related files closest first (a stable order: files equally close keep the order they
/// were found in), each import whose names `unit_text` uses moved up to `UsedByTheUnit`.
///
/// Example: a command importing `Booking` and `Clock` whose `handle` calls `self.clock.now()`
/// puts the `Clock` port first, then `Booking`, then the files beside them.
pub fn closest_first(files: Vec<RelatedFile>, unit_text: &str) -> Vec<RelatedFile> {
    let mut files: Vec<RelatedFile> = files
        .into_iter()
        .map(|file| {
            let used = file.closeness == Closeness::Imported
                && file.names.iter().any(|name| mentions_word(unit_text, name));
            if used {
                RelatedFile {
                    closeness: Closeness::UsedByTheUnit,
                    ..file
                }
            } else {
                file
            }
        })
        .collect();
    files.sort_by_key(|file| file.closeness);
    files
}

/// Whether `word` appears in `text` as a whole word: not inside a longer name.
///
/// Example: `Clock` is in `impl Clock for InMemoryClock`, but not in `ClockFace` or `clock`.
pub fn mentions_word(text: &str, word: &str) -> bool {
    let is_name_char = |c: char| c.is_alphanumeric() || c == '_';
    !word.is_empty()
        && text.match_indices(word).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let after = text[at + word.len()..].chars().next();
            !before.is_some_and(is_name_char) && !after.is_some_and(is_name_char)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn related(path: &str, chars: usize, closeness: Closeness, names: &[&str]) -> RelatedFile {
        RelatedFile {
            path: path.into(),
            text: "x".repeat(chars),
            closeness,
            names: names.iter().map(|n| n.to_string()).collect(),
        }
    }

    #[test]
    fn a_file_that_does_not_fit_is_left_out_whole_and_a_smaller_one_still_comes_in() {
        let files = vec![
            related("a", 240, Closeness::Imported, &[]),
            related("b", 200, Closeness::Imported, &[]),
            related("c", 120, Closeness::Imported, &[]),
        ];
        let chosen = chosen_within(files, 100);
        let paths = |files: &[RelatedFile]| -> Vec<String> {
            files
                .iter()
                .map(|f| f.path.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(paths(&chosen.kept), ["a", "c"]);
        assert_eq!(paths(&chosen.left_for_room), ["b"]);
        assert_eq!(chosen.kept[0].text.len(), 240, "kept whole");
    }

    #[test]
    fn an_import_the_unit_uses_comes_first_then_imports_then_files_near_them() {
        let files = vec![
            related("ports/declarations", 1, Closeness::DeclaringAnImport, &[]),
            related("booking", 1, Closeness::Imported, &["Booking"]),
            related("clock/in_memory", 1, Closeness::BesideAnImport, &[]),
            related("clock/port", 1, Closeness::Imported, &["Clock"]),
        ];
        let order: Vec<_> = closest_first(files, "fn handle(&self, clock: &dyn Clock) {}")
            .into_iter()
            .map(|f| f.path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            order,
            [
                "clock/port",
                "booking",
                "clock/in_memory",
                "ports/declarations"
            ]
        );
    }

    #[test]
    fn a_word_counts_only_whole() {
        assert!(mentions_word("impl Clock for InMemoryClock", "Clock"));
        assert!(!mentions_word("struct ClockFace; let clock = 1;", "Clock"));
        assert!(!mentions_word("anything", ""));
    }
}
