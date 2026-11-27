//! The person's edits as the agent is told them: one unified diff per file they changed since
//! the task last looked, under a sentence that says their edits win.

use similar::TextDiff;

/// One file the person changed since the last look: its path relative to the root (`/`), its
/// text at the last look and now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEdit {
    pub path: String,
    pub before: String,
    pub after: String,
}

/// A file's edit as a unified diff with two lines of context.
///
/// Example: `a.txt` from `x = 1` to `x = 2` is `--- a/a.txt`, `+++ b/a.txt`, `@@ -1 +1 @@`,
/// `-x = 1`, `+x = 2`.
pub fn diff_text(edit: &FileEdit) -> String {
    TextDiff::from_lines(&edit.before, &edit.after)
        .unified_diff()
        .context_radius(2)
        .header(&format!("a/{}", edit.path), &format!("b/{}", edit.path))
        .to_string()
}

/// What the agent is told after a tool call when the person changed files meanwhile; `None`
/// when they changed nothing, so a quiet editor costs the agent no tokens.
pub fn edits_context_text(edits: &[FileEdit]) -> Option<String> {
    let changed: Vec<&FileEdit> = edits.iter().filter(|e| e.before != e.after).collect();
    if changed.is_empty() {
        return None;
    }
    let diffs: String = changed.iter().map(|edit| diff_text(edit)).collect();
    Some(format!(
        "The person changed these files while you worked; their edits are authoritative: keep them, build on them, and never write back over them.\n\n{diffs}"
    ))
}

/// What the agent is told at the start about open files whose text in the editor is not yet
/// saved: `Read` shows the disk, the editor holds these edits.
pub fn unsaved_edits_text(edits: &[FileEdit]) -> Option<String> {
    let changed: Vec<&FileEdit> = edits.iter().filter(|e| e.before != e.after).collect();
    if changed.is_empty() {
        return None;
    }
    let diffs: String = changed.iter().map(|edit| diff_text(edit)).collect();
    Some(format!(
        "These files have edits in the person's editor that are not saved yet (Read shows the disk; the editor's text is the disk with these diffs applied, and it is what write_file must build on):\n\n{diffs}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quiet_editor_says_nothing() {
        let same = FileEdit {
            path: "a.txt".into(),
            before: "x\n".into(),
            after: "x\n".into(),
        };
        assert_eq!(edits_context_text(&[]), None);
        assert_eq!(edits_context_text(&[same]), None);
    }

    #[test]
    fn an_edit_is_a_unified_diff_under_the_rule() {
        let edit = FileEdit {
            path: "src/a.txt".into(),
            before: "one\nTooLate\nthree\n".into(),
            after: "one\nTooLateToCancel\nthree\n".into(),
        };
        let text = edits_context_text(&[edit]).unwrap();
        assert!(text.starts_with("The person changed these files"), "{text}");
        assert!(
            text.contains("--- a/src/a.txt\n+++ b/src/a.txt\n"),
            "{text}"
        );
        assert!(text.contains("-TooLate\n+TooLateToCancel\n"), "{text}");
    }
}
