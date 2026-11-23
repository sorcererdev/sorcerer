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
        /// Print the findings as JSON (for an agent or a script)
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
