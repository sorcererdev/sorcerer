//! Reading the writer's reply: `<goal>…</goal>` and one `<file path="…">…</file>` block per file.

/// What the writer answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeReply {
    /// The one-sentence goal, when the reply has a `<goal>` block.
    pub goal: Option<String>,
    /// Each file the writer wrote: the path as it wrote it, and the file's whole new text.
    pub files: Vec<(String, String)>,
}

/// Reads a reply. Blocks may sit anywhere in the text; a markdown fence around a file's body is
/// taken off; one newline at each end of the body is trimmed and a final newline ensured; a
/// block never closed is left out. When the same path is answered twice, the last block wins.
///
/// Example: `<goal>fill it</goal>\n<file path="a.txt">\nhi\n</file>` is the goal `fill it` and
/// one file, `a.txt` with the text `hi\n`.
pub fn read_reply(text: &str) -> ChangeReply {
    ChangeReply {
        goal: goal_of(text),
        files: last_block_per_path(file_blocks(text)),
    }
}

impl ChangeReply {
    /// Whether the writer found nothing to do: its goal is `nothing` (any case) or it wrote no
    /// file.
    pub fn is_nothing(&self) -> bool {
        let says_nothing = self
            .goal
            .as_ref()
            .is_some_and(|goal| goal.eq_ignore_ascii_case("nothing"));
        says_nothing || self.files.is_empty()
    }
}

fn goal_of(text: &str) -> Option<String> {
    let (_, after_open) = text.split_once("<goal>")?;
    let goal = after_open.split("</goal>").next().unwrap_or(after_open);
    Some(goal.trim().to_string())
}

/// Every closed `<file path="…">…</file>` block, in the order written.
fn file_blocks(text: &str) -> Vec<(String, String)> {
    text.split("<file path=\"")
        .skip(1)
        .filter_map(|chunk| {
            let (path, rest) = chunk.split_once("\">")?;
            let (body, _) = rest.split_once("</file>")?;
            Some((path.trim().to_string(), body_of_file(body)))
        })
        .collect()
}

/// A block's body as the file's text: a fence around it taken off, one newline trimmed at each
/// end, a newline at the end ensured.
fn body_of_file(body: &str) -> String {
    let trimmed = body.strip_prefix('\n').unwrap_or(body);
    let trimmed = trimmed.strip_suffix('\n').unwrap_or(trimmed);
    let mut text = unfenced(trimmed);
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// The lines inside a markdown fence when the body is wrapped in one, else the body.
fn unfenced(body: &str) -> String {
    let lines: Vec<&str> = body.lines().collect();
    match lines.as_slice() {
        [first, middle @ .., last]
            if first.trim().starts_with("```") && last.trim().starts_with("```") =>
        {
            middle.join("\n")
        }
        _ => body.to_string(),
    }
}

/// A path answered twice keeps its last block, at the place of that last block.
fn last_block_per_path(blocks: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut kept: Vec<(String, String)> = Vec::new();
    for (path, body) in blocks {
        kept.retain(|(earlier, _)| *earlier != path);
        kept.push((path, body));
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_anywhere_fences_unwrapped_newline_ensured() {
        let reply = read_reply(
            "Sure!\n<goal> fill it </goal> and\n<file path=\" a.txt \">\n```text\nhi\n```\n</file>\nthen\n<file path=\"b.txt\">no newline</file>\n<file path=\"never closed\">x",
        );
        assert_eq!(reply.goal.as_deref(), Some("fill it"));
        assert_eq!(
            reply.files,
            [
                ("a.txt".to_string(), "hi\n".to_string()),
                ("b.txt".to_string(), "no newline\n".to_string())
            ]
        );
    }

    #[test]
    fn the_same_path_twice_keeps_the_last() {
        let reply = read_reply(
            "<goal>g</goal><file path=\"a\">\none\n</file><file path=\"b\">\nb\n</file><file path=\"a\">\ntwo\n</file>",
        );
        assert_eq!(
            reply.files,
            [
                ("b".to_string(), "b\n".to_string()),
                ("a".to_string(), "two\n".to_string())
            ]
        );
    }

    #[test]
    fn goal_nothing_is_nothing() {
        assert!(read_reply("<goal>Nothing</goal>").is_nothing());
        assert!(read_reply("<goal>did a thing</goal>").is_nothing());
        assert!(read_reply("no blocks at all").is_nothing());
        assert!(!read_reply("<goal>g</goal><file path=\"a\">\nx\n</file>").is_nothing());
        assert!(read_reply("<goal>NOTHING</goal><file path=\"a\">\nx\n</file>").is_nothing());
    }
}
