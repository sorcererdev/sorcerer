//! Which commands the agent may run without asking: the exact command lines the person allowed
//! *always for this project*. A different command line is asked about again, so allowing
//! `grep KEY src/app.txt` never lets `grep -r KEY .` run. *Always* is never offered for a
//! command that chains others (`&&`, `;`, `|`, a redirect, a subshell), reads a whole tree
//! (`TreeReaders`) or has a glob in an argument.

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::versioned::Versioned;

use crate::domain::tree_readers::TreeReaders;

/// The command lines allowed always, as `permissions.json` keeps them (version 2), each with
/// its words joined by one space.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllowedCommands {
    pub commands: Vec<String>,
}

impl Versioned for AllowedCommands {
    const VERSION: u32 = 2;
}

impl AllowedCommands {
    /// Whether the command may run unasked: it is one of the allowed command lines (spaces
    /// aside), and it could still be allowed always (a hand-edited file cannot let a tree
    /// reader through).
    ///
    /// Example: with `just test -q` allowed, `just test  -q` runs unasked; `uv run
    /// pytest tests` and `just test -q; rm x` are asked about.
    pub fn allows(&self, command: &str, readers: &TreeReaders) -> bool {
        always_for(command, readers).is_some_and(|line| self.commands.contains(&line))
    }

    /// The same list with one more command line (once).
    pub fn with(&self, line: &str) -> AllowedCommands {
        let mut commands = self.commands.clone();
        if !commands.iter().any(|known| known == line) {
            commands.push(line.to_string());
        }
        AllowedCommands { commands }
    }
}

/// `permissions.json` as version 1 kept it: the first words of each command allowed always
/// (`just test`, `grep`), which let any command starting with them run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandPrefixes {
    pub allowed: Vec<String>,
}

impl Versioned for CommandPrefixes {
    const VERSION: u32 = 1;
}

impl CommandPrefixes {
    /// The version 2 list: each prefix of two words or more becomes that exact command line
    /// when it could be allowed always now; a bare program name (`grep`, `ls`) is dropped, as
    /// is a tree reader (`git show`). The dropped ones are returned too, to be said.
    ///
    /// Example: `["just test", "grep", "git show"]` keeps `just test` and drops `grep`
    /// and `git show`.
    pub fn migrated(&self, readers: &TreeReaders) -> (AllowedCommands, Vec<String>) {
        let (kept, dropped): (Vec<&String>, Vec<&String>) =
            self.allowed.iter().partition(|prefix| {
                prefix.split_whitespace().count() >= 2 && always_for(prefix, readers).is_some()
            });
        let commands = kept
            .into_iter()
            .fold(AllowedCommands::default(), |list, prefix| {
                list.with(&one_line(prefix))
            });
        (commands, dropped.into_iter().cloned().collect())
    }
}

/// The command line *always* would remember: the command with its words joined by one space.
/// `None` when it may only be allowed once: it chains others, reads a whole tree, or has a
/// glob (`*`, `?`, `[`) in an argument.
///
/// Example: `just test  -q` remembers `just test -q`; `grep -r KEY .`, `cat s*/x` and
/// `make && rm x` remember nothing.
pub fn always_for(command: &str, readers: &TreeReaders) -> Option<String> {
    let line = one_line(command);
    let once_only =
        line.is_empty() || chains_commands(&line) || has_glob(&line) || readers.reads_tree(&line);
    (!once_only).then_some(line)
}

fn one_line(command: &str) -> String {
    command.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn has_glob(line: &str) -> bool {
    line.split_whitespace()
        .skip(1)
        .any(|word| word.contains(['*', '?', '[']))
}

/// The words of a command that could name a file: the command split at spaces, quotes, shell
/// operators and the `:` and `=` that join a path to something else.
///
/// Example: `git show HEAD:.env --output=out.txt` gives `git`, `show`, `HEAD`, `.env`,
/// `--output` and `out.txt`.
pub fn words_naming_paths(command: &str) -> impl Iterator<Item = &str> {
    command
        .split(|c: char| c.is_whitespace() || "'\"`;|&<>()=:".contains(c))
        .filter(|word| !word.is_empty())
}

fn chains_commands(command: &str) -> bool {
    ["&&", "||", ";", "|", ">", "<", "`", "$(", "\n"]
        .iter()
        .any(|operator| command.contains(operator))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readers() -> &'static TreeReaders {
        TreeReaders::shipped()
    }

    #[test]
    fn a_windows_spelling_of_a_tree_reader_is_never_allowed_always() {
        for command in ["rg.exe KEY", "GREP.EXE -r KEY .", "C:\\tools\\rg.exe KEY"] {
            assert_eq!(always_for(command, readers()), None, "{command}");
            let hand_edited = AllowedCommands::default().with(command);
            assert!(!hand_edited.allows(command, readers()), "{command}");
        }
        assert_eq!(
            always_for("uv.exe run pytest -q", readers()).as_deref(),
            Some("uv.exe run pytest -q")
        );
    }

    #[test]
    fn only_the_exact_command_line_runs_unasked() {
        let allowed = AllowedCommands::default().with("grep KEY src");
        assert!(allowed.allows("grep KEY src", readers()));
        assert!(allowed.allows("grep  KEY   src", readers()));
        assert!(!allowed.allows("grep -r KEY .", readers()));
        assert!(!allowed.allows("grep KEY src other", readers()));
        assert!(!allowed.allows("grep KEY src; rm -rf x", readers()));
    }

    #[test]
    fn always_is_never_offered_for_a_chain_a_tree_reader_or_a_glob() {
        assert_eq!(
            always_for("just test  -q", readers()).as_deref(),
            Some("just test -q")
        );
        assert_eq!(
            always_for("grep KEY src/", readers()).as_deref(),
            Some("grep KEY src/")
        );
        for command in [
            "grep -r KEY .",
            "rg KEY",
            "git log -p -1",
            "git show HEAD",
            "cat s*/keys.txt",
            "find . -exec cat {} ;",
            "ls | xargs cat",
            "make && rm x",
            "make > out",
            "",
        ] {
            assert_eq!(always_for(command, readers()), None, "{command}");
        }
    }

    #[test]
    fn a_hand_edited_tree_reader_is_not_allowed() {
        let allowed = AllowedCommands {
            commands: vec!["grep -r KEY .".into()],
        };
        assert!(!allowed.allows("grep -r KEY .", readers()));
    }

    #[test]
    fn version_1_keeps_its_command_lines_and_drops_bare_programs_and_tree_readers() {
        let old = CommandPrefixes {
            allowed: vec![
                "just test".into(),
                "grep".into(),
                "make test".into(),
                "git show".into(),
                "ls".into(),
            ],
        };
        let (new, dropped) = old.migrated(readers());
        assert_eq!(new.commands, ["just test", "make test"]);
        assert_eq!(dropped, ["grep", "git show", "ls"]);
    }

    #[test]
    fn a_path_joined_by_a_colon_an_equals_or_a_quote_is_a_word_of_its_own() {
        let words: Vec<&str> =
            words_naming_paths("git show HEAD:.env --output='out.txt'|cat<x").collect();
        assert_eq!(
            words,
            [
                "git", "show", "HEAD", ".env", "--output", "out.txt", "cat", "x"
            ]
        );
    }
}
