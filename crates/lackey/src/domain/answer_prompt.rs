//! The question a research call answers, and reading its reply: one to three sentences with
//! the code cited as `path:line`; a reply that says it is long starts with `LONG:`.

use crate::domain::region::Region;

/// The research prompt for a question the person wrote.
pub fn question_prompt(region: &Region) -> String {
    let unit = region
        .unit
        .as_ref()
        .map(|unit| format!(", in {unit}"))
        .unwrap_or_default();
    format!(
        "The person wrote this question in {} at line {}{unit}. Answer it about this repository in one to three sentences, citing the code your answer rests on as path:line with the path relative to the repository root. If a good answer needs more than three sentences, start your reply with LONG: and give the whole answer in markdown.\n\n{}",
        region.file,
        region.lines.start,
        region.text.trim()
    )
}

/// A reply read: its text (without `LONG:`), whether it is long, and every `path:line` cited
/// that looks like a file (it has a `.` or a `/`).
///
/// Example: `handle deletes any booking (src/cancel.txt:31).` cites `("src/cancel.txt", 31)`.
pub fn read_answer(reply: &str) -> (String, bool, Vec<(String, u32)>) {
    let trimmed = reply.trim();
    let (text, long) = match trimmed.strip_prefix("LONG:") {
        Some(rest) => (rest.trim().to_string(), true),
        None => (trimmed.to_string(), false),
    };
    (text.clone(), long, citations_in(&text))
}

fn citations_in(text: &str) -> Vec<(String, u32)> {
    let mut found: Vec<(String, u32)> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || "()[],;`'\"".contains(c)) {
        let Some((path, line)) = word.trim_end_matches(['.', ':']).rsplit_once(':') else {
            continue;
        };
        let line: String = line.chars().take_while(char::is_ascii_digit).collect();
        let Ok(line) = line.parse::<u32>() else {
            continue;
        };
        if line > 0
            && path.contains(['.', '/'])
            && !found.iter().any(|(p, l)| p == path && *l == line)
        {
            found.push((path.to_string(), line));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citations_are_read_and_long_is_said() {
        let (text, long, cited) = read_answer(
            "LONG: It deletes any booking (src/cancel.txt:31) and see `docs/a.md:4-9`.",
        );
        assert!(long);
        assert!(text.starts_with("It deletes"));
        assert_eq!(
            cited,
            [
                ("src/cancel.txt".to_string(), 31),
                ("docs/a.md".to_string(), 4)
            ]
        );
        let (_, long, cited) = read_answer("At 3:30 nothing. See main:12.");
        assert!(!long);
        assert!(cited.is_empty(), "{cited:?}");
    }
}
