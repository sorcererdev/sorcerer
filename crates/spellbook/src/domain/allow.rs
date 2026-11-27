//! Allow comments: a person silences one rule at one place with a reason.
//!
//! Written in the file's own comment syntax, on the line of the finding or the line above:
//! `# spellbook:allow commands-use-ports — migration, tracked in issue 12`. The reason follows
//! an em dash, ` - ` or ` -- `. In a language whose comments close, the comment's end closes
//! the reason: `<!-- spellbook:allow a-rule — why -->`.

use sorcerer_core::domain::language::CommentMarks;

/// One allow comment found in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllowComment {
    /// The line it is on (from 1).
    pub line: u32,
    /// The rule it names.
    pub rule: String,
    pub has_reason: bool,
}

const MARK: &str = "spellbook:allow";

/// One comment of a file, as the parser found it (so text inside a string is never one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// The line the comment starts on (from 1).
    pub line: u32,
    /// Its text, from its comment marker to its end.
    pub text: String,
}

/// Every allow comment among a file's comments, given how its language writes a comment.
///
/// Example: with `#`, the comment `# spellbook:allow a-rule — why` is an allow of `a-rule`
/// with a reason; `# spellbook:allow a-rule` is one without; with `<!--` and `-->`,
/// `<!-- spellbook:allow a-rule -->` is one without too.
pub fn allow_comments(comments: &[Comment], marks: &CommentMarks) -> Vec<AllowComment> {
    comments
        .iter()
        .flat_map(|comment| {
            comment
                .text
                .lines()
                .enumerate()
                .filter_map(|(offset, line)| {
                    let line_number = comment
                        .line
                        .saturating_add(u32::try_from(offset).unwrap_or(0));
                    allow_in_line(line, marks, line_number)
                })
        })
        .collect()
}

fn allow_in_line(line: &str, marks: &CommentMarks, line_number: u32) -> Option<AllowComment> {
    let at = line.find(MARK)?;
    // The comment marker comes right before the word (a `#` inside a string earlier is not it).
    if !line[..at].trim_end().ends_with(marks.start.as_str()) {
        return None;
    }
    let after_mark = marks.without_end(line[at + MARK.len()..].trim_start());
    let rule: String = after_mark
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    Some(AllowComment {
        line: line_number,
        has_reason: has_a_reason(after_mark[rule.len()..].trim()),
        rule,
    })
}

/// Whether what follows the rule is a separator and then some words.
fn has_a_reason(after_rule: &str) -> bool {
    ["—", "--", "-"].iter().any(|separator| {
        after_rule
            .strip_prefix(separator)
            .is_some_and(|reason| !reason.trim().is_empty())
    })
}

/// The allow comment that silences a finding: on its line or the line above, naming its rule.
pub fn silencing<'c>(
    comments: &'c [AllowComment],
    rule: &str,
    line: u32,
) -> Option<&'c AllowComment> {
    comments
        .iter()
        .find(|c| c.rule == rule && (c.line == line || c.line + 1 == line))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The comments of a file whose every line is a comment or code, by its `#`s (a test double
    /// for the parser: a line is a comment from its `#`, `//`, `<!--` or nothing).
    fn comments_of(text: &str) -> Vec<Comment> {
        text.lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let start = line
                    .find("//")
                    .or_else(|| line.find('#'))
                    .or_else(|| line.find("<!--"))?;
                Some(Comment {
                    line: u32::try_from(index + 1).unwrap(),
                    text: line[start..].to_string(),
                })
            })
            .collect()
    }

    fn allow_comments(text: &str, marker: &str) -> Vec<AllowComment> {
        super::allow_comments(&comments_of(text), &CommentMarks::line(marker))
    }

    #[test]
    fn a_reason_follows_a_dash() {
        let text = "import x  # spellbook:allow commands-use-ports — migration\n";
        assert_eq!(
            allow_comments(text, "#"),
            [AllowComment {
                line: 1,
                rule: "commands-use-ports".into(),
                has_reason: true
            }]
        );
        for separator in [" - ", " -- "] {
            let line = format!("# spellbook:allow a{separator}because");
            assert!(allow_comments(&line, "#")[0].has_reason, "{line}");
        }
    }

    #[test]
    fn no_reason_when_nothing_follows_the_dash_or_the_rule() {
        assert!(!allow_comments("# spellbook:allow a", "#")[0].has_reason);
        assert!(!allow_comments("# spellbook:allow a —", "#")[0].has_reason);
        assert!(!allow_comments("# spellbook:allow a — ", "#")[0].has_reason);
    }

    #[test]
    fn other_comments_are_not_allows() {
        assert!(allow_comments("# a note about spellbook:allow", "#").is_empty());
        assert!(allow_comments("x = 1", "#").is_empty());
        assert!(allow_comments("// spellbook:allow a — r", "#").is_empty());
        assert_eq!(allow_comments("// spellbook:allow a — r", "//").len(), 1);
    }

    #[test]
    fn an_allow_silences_its_line_and_the_next() {
        let comments = allow_comments("# spellbook:allow a — r\nimport x\n\nimport y\n", "#");
        assert!(silencing(&comments, "a", 1).is_some());
        assert!(silencing(&comments, "a", 2).is_some());
        assert!(silencing(&comments, "a", 4).is_none());
        assert!(silencing(&comments, "b", 2).is_none());
    }

    #[test]
    fn a_closed_comment_ends_the_reason() {
        let marks = CommentMarks {
            start: "<!--".into(),
            end: Some("-->".into()),
        };
        let found = super::allow_comments(
            &comments_of("<!-- spellbook:allow a — why -->\n<!-- spellbook:allow b -->\n"),
            &marks,
        );
        assert_eq!(found.len(), 2);
        assert!(found[0].has_reason && found[0].rule == "a");
        assert!(!found[1].has_reason && found[1].rule == "b");
    }
}
