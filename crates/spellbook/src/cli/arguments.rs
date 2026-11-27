//! The command line's grammar: the arguments and subcommands `spellbook` accepts.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "spellbook",
    version,
    about = "A project's conventions as data: generators with holes"
)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) cmd: Cmd,
}

#[derive(Subcommand, Debug)]
pub(super) enum Cmd {
    /// The project's generators and the inputs each asks for
    List,
    /// One generator: its inputs and their choices now, its files, its holes, its examples
    Show { generator: String },
    /// Write every file of a generator after showing them; --plan only shows them
    New {
        generator: String,
        /// An input's value (repeat it for each input)
        #[arg(long = "set", value_name = "INPUT=VALUE")]
        set: Vec<String>,
        /// Show the files and holes, write nothing
        #[arg(long)]
        plan: bool,
        /// Print what happened as JSON (for an agent or a script)
        #[arg(long)]
        json: bool,
    },
    /// Fix the finding on a line with one of its rule's fixes, after showing the change;
    /// --plan only shows it
    Fix {
        /// The finding's place, FILE:LINE
        #[arg(value_name = "FILE:LINE")]
        at: String,
        /// The rule, when two rules find something on the line
        #[arg(long)]
        rule: Option<String>,
        /// Which fix: its number from 1, or its label (the first when not given)
        #[arg(long = "fix", value_name = "N|LABEL")]
        fix: Option<String>,
        /// An input's value (repeat it for each input)
        #[arg(long = "set", value_name = "INPUT=VALUE")]
        set: Vec<String>,
        /// Show the change, write nothing
        #[arg(long)]
        plan: bool,
        /// Print the plan (or what was applied) as JSON
        #[arg(long)]
        json: bool,
    },
    /// The holes still open: placeholders a generation left that nobody has filled yet
    Holes {
        /// Only the holes in these files
        files: Vec<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Run the project's rules over the project, or only the named files
    Check {
        /// Only these files
        files: Vec<PathBuf>,
        /// Only the findings on lines changed since this commit (and every region)
        #[arg(long, value_name = "REV", conflicts_with = "files")]
        since: Option<String>,
        /// Record today's findings as known debt in .sorcerer/spellbook/baseline.toml
        #[arg(long, conflicts_with_all = ["files", "since"])]
        baseline: bool,
        /// Print the findings as JSON (for an agent or a script)
        #[arg(long)]
        json: bool,
    },
    /// Refresh the owned regions (the lists spellbook keeps between their markers) from the
    /// files on disk
    Sync {
        /// Set up the region NAME in every file it matches that has no markers for it yet
        #[arg(long, value_name = "NAME")]
        init: Option<String>,
        /// Write nothing; exit 1 when a region is out of date or not set up (for hooks and CI)
        #[arg(long)]
        check: bool,
        /// Show the refresh, write nothing
        #[arg(long)]
        plan: bool,
        /// Print the plan (or what was applied) as JSON
        #[arg(long)]
        json: bool,
    },
    /// Whether spellbook can run here: generators, languages and their holes, conjure, trust,
    /// the records
    Doctor,
    /// The editor's language server, over standard input and output
    Lsp,
    /// Let the editor's server run spellbook in this project although its config runs code
    Trust {
        /// Take the project off the trust list instead
        #[arg(long)]
        revoke: bool,
    },
}
