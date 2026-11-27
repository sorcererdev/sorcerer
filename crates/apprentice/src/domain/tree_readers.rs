//! The commands that read whole trees of files (`grep -r`, `rg`, `git log -p`), which are never
//! allowed *always*: a search of the project reads a fenced file without naming it. The list is
//! data, `library/tree-readers.toml`, since sorcerer's code names no tool.

use std::sync::LazyLock;

use serde::Deserialize;

/// The shipped list, `library/tree-readers.toml`.
const SHIPPED: &str = include_str!("../../library/tree-readers.toml");

static SHIPPED_READERS: LazyLock<TreeReaders> =
    LazyLock::new(|| TreeReaders::from_toml(SHIPPED).expect("library/tree-readers.toml parses"));

/// The programs that read trees, and when.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeReaders {
    #[serde(rename = "reader", default)]
    readers: Vec<Reader>,
}

/// One program (with its subcommand, for one that has them) that reads a tree always, or when
/// given one of its options.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reader {
    program: String,
    subcommand: Option<String>,
    #[serde(default)]
    always: bool,
    #[serde(default)]
    options: Vec<String>,
}

impl TreeReaders {
    /// The list apprentice ships.
    pub fn shipped() -> &'static TreeReaders {
        &SHIPPED_READERS
    }

    /// The list from TOML text in the shape of `library/tree-readers.toml`.
    pub fn from_toml(text: &str) -> Result<TreeReaders, toml::de::Error> {
        toml::from_str(text)
    }

    /// Whether the command reads a tree: some word of it is a listed program (by its file
    /// name, so `/usr/bin/grep`, `env grep` and Windows' `GREP.EXE` count), and the words after
    /// it make it one.
    ///
    /// Example: `grep -rn KEY .` and `git log -p` read trees; `grep KEY src/app.txt`, `git log
    /// --oneline` and `make test` do not.
    pub fn reads_tree(&self, command: &str) -> bool {
        let words: Vec<&str> = command.split_whitespace().collect();
        (0..words.len()).any(|at| {
            let program = program_name(words[at]);
            let rest = &words[at + 1..];
            self.readers
                .iter()
                .any(|reader| reader.program == program && reader.reads_tree(rest))
        })
    }
}

impl Reader {
    /// Whether the program's arguments make it read a tree.
    fn reads_tree(&self, arguments: &[&str]) -> bool {
        let arguments = match &self.subcommand {
            None => arguments,
            Some(subcommand) => match first_non_option(arguments) {
                Some(at) if arguments[at] == subcommand => &arguments[at + 1..],
                _ => return false,
            },
        };
        self.always
            || arguments
                .iter()
                .any(|argument| self.options.iter().any(|option| matches(option, argument)))
    }
}

/// Suffixes Windows runs a program by, which name no tool themselves.
const PROGRAM_SUFFIXES: [&str; 4] = [".exe", ".cmd", ".bat", ".com"];

/// The program a word runs, as the list names it: its file name (a path to it counts), without
/// quotes, without a Windows program suffix, in lower case. Lower case is right on Windows and
/// only stricter elsewhere: a `GREP` that is not grep is merely never allowed always.
///
/// Example: `C:\tools\RG.EXE`, `"rg.exe"` and `/usr/bin/rg` all name `rg`.
fn program_name(word: &str) -> String {
    let unquoted = word.trim_matches(['"', '\'']);
    let file = unquoted.rsplit(['/', '\\']).next().unwrap_or(unquoted);
    let lower = file.to_lowercase();
    PROGRAM_SUFFIXES
        .iter()
        .find_map(|suffix| lower.strip_suffix(suffix))
        .filter(|name| !name.is_empty())
        .map_or_else(|| lower.clone(), str::to_string)
}

/// Where the subcommand is: the first argument that is not an option (`git -C x log` is
/// not handled: an option's value counts as the subcommand, so such a command reads as no
/// listed subcommand and is merely asked about each time it is not allowed always).
fn first_non_option(arguments: &[&str]) -> Option<usize> {
    arguments
        .iter()
        .position(|argument| !argument.starts_with('-'))
}

/// Whether an argument is the option: the same word, `--name=value` for `--name`, or a
/// cluster of short options holding the letter for `-x` (`-rn` holds `-r`).
fn matches(option: &str, argument: &str) -> bool {
    if argument == option {
        return true;
    }
    if option.starts_with("--") {
        return argument
            .strip_prefix(option)
            .is_some_and(|rest| rest.starts_with('='));
    }
    let short_letter = option
        .strip_prefix('-')
        .filter(|letter| letter.chars().count() == 1);
    match short_letter {
        Some(letter) => {
            argument.starts_with('-')
                && !argument.starts_with("--")
                && argument[1..].contains(letter)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_list_parses_and_names_the_recursive_searches() {
        let readers = TreeReaders::shipped();
        for command in [
            "grep -r KEY .",
            "grep -rn KEY .",
            "grep --recursive KEY",
            "/usr/bin/grep -R KEY .",
            "env LC_ALL=C grep -r KEY .",
            "rg KEY",
            "rg --hidden KEY",
            "git grep KEY",
            "git log -p -1",
            "git log --patch",
            "git show HEAD:a",
            "find . -name x -exec cat {} +",
            "xargs cat",
            "cp -r . /tmp/x",
            // Windows spells the program with its suffix, in any case, often quoted
            "rg.exe KEY",
            "RG KEY",
            "GREP.EXE -r KEY .",
            "C:\\tools\\grep.exe -r KEY .",
            "\"C:\\Program Files\\ripgrep\\rg.exe\" KEY",
            "git.cmd grep KEY",
            "Git.Bat log -p",
            "xargs.com cat",
        ] {
            assert!(readers.reads_tree(command), "{command}");
        }
        for command in [
            "grep KEY src/app.txt",
            "grep -n KEY src/app.txt",
            "git log --oneline",
            "git status",
            "find . -name x",
            "cp a b",
            "make test",
            "grep.exe KEY src/app.txt",
            "rgx KEY",
            "rg.exe.txt KEY",
        ] {
            assert!(!readers.reads_tree(command), "{command}");
        }
    }

    #[test]
    fn a_program_is_named_without_its_path_suffix_or_case() {
        for (word, name) in [
            ("grep", "grep"),
            ("/usr/bin/grep", "grep"),
            ("C:\\tools\\RG.EXE", "rg"),
            ("git.cmd", "git"),
            ("\"rg.exe\"", "rg"),
            ("'x.bat'", "x"),
            ("tar.gz", "tar.gz"),
        ] {
            assert_eq!(program_name(word), name, "{word}");
        }
    }
}
