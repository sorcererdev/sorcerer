//! Which commands the agent may run without asking: the ones whose first words the person
//! allowed *always for this project*. A command that chains others (`&&`, `;`, `|`, a
//! redirect, a subshell) is never allowed by its first words, so `just test && rm -rf x`
//! is asked about however `just test` was allowed.

use serde::{Deserialize, Serialize};
use sorcerer_core::domain::versioned::Versioned;

/// The command prefixes allowed always, as `permissions.json` keeps them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandPrefixes {
    pub allowed: Vec<String>,
}

impl Versioned for CommandPrefixes {
    const VERSION: u32 = 1;
}

/// The most words a remembered prefix has.
const PREFIX_WORDS: usize = 3;

impl CommandPrefixes {
    /// Whether the command may run unasked: it chains nothing and its words begin with an
    /// allowed prefix's words.
    ///
    /// Example: with `just test` allowed, `just test -q tests` runs unasked, `uv run
    /// python x` and `just test; rm x` are asked about.
    pub fn allows(&self, command: &str) -> bool {
        if chains_commands(command) {
            return false;
        }
        let words: Vec<&str> = command.split_whitespace().collect();
        self.allowed.iter().any(|prefix| {
            let prefix: Vec<&str> = prefix.split_whitespace().collect();
            !prefix.is_empty() && words.starts_with(&prefix)
        })
    }

    /// The same list with one more prefix (once).
    pub fn with(&self, prefix: &str) -> CommandPrefixes {
        let mut allowed = self.allowed.clone();
        if !allowed.iter().any(|known| known == prefix) {
            allowed.push(prefix.to_string());
        }
        CommandPrefixes { allowed }
    }
}

/// The prefix *always* remembers for a command: its first words, up to three, stopping before
/// the first that is an option or names a path. `None` for a command that chains others: such a
/// command can be allowed once, never always.
///
/// Example: `just test tests/test_x.txt -q` remembers `just test`.
pub fn prefix_of(command: &str) -> Option<String> {
    if chains_commands(command) {
        return None;
    }
    let words: Vec<&str> = command
        .split_whitespace()
        .take_while(|word| !word.starts_with('-') && !word.contains(['/', '\\', '.', '=']))
        .take(PREFIX_WORDS)
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

fn chains_commands(command: &str) -> bool {
    ["&&", "||", ";", "|", ">", "<", "`", "$(", "\n"]
        .iter()
        .any(|operator| command.contains(operator))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_allowed_prefix_lets_its_commands_run() {
        let prefixes = CommandPrefixes::default().with("just test");
        assert!(prefixes.allows("just test"));
        assert!(prefixes.allows("just test -q tests"));
        assert!(!prefixes.allows("just lint x"));
        assert!(!prefixes.allows("just test; rm -rf x"));
        assert!(!prefixes.allows("just test && rm -rf x"));
        assert!(!prefixes.allows("just test > out"));
    }

    #[test]
    fn the_remembered_prefix_stops_at_an_option_or_a_path() {
        assert_eq!(
            prefix_of("just test tests/test_x.txt -q").as_deref(),
            Some("just test")
        );
        assert_eq!(prefix_of("make test").as_deref(), Some("make test"));
        assert_eq!(prefix_of("ls -la").as_deref(), Some("ls"));
        assert_eq!(prefix_of("a && b"), None);
        assert_eq!(prefix_of("./run.sh"), None);
    }

    #[test]
    fn a_prefix_is_kept_once() {
        let prefixes = CommandPrefixes::default().with("make").with("make");
        assert_eq!(prefixes.allowed, ["make"]);
    }
}
