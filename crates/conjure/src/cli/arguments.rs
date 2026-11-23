//! The command line's grammar: the arguments and subcommands `conjure` accepts.

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "conjure",
    version,
    about = "Writes one small change where you ask",
    args_conflicts_with_subcommands = true
)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) cmd: Option<Cmd>,
    #[command(flatten)]
    pub(super) run: RunArgs,
}

#[derive(Subcommand, Debug)]
pub(super) enum Cmd {
    /// Whether conjure can run here: git, provider, settings, languages, scry's findings, records
    Doctor,
    /// What conjure spent, a local day a row
    Cost {
        #[arg(long, value_name = "YYYY-MM-DD")]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// The last changes the editor recorded, newest first
    Log {
        /// How many (10 by default)
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        n: Option<u32>,
    },
    /// Let the editor conjure in this project although its config runs code or moves your key
    Trust {
        /// Take the project off the trust list instead
        #[arg(long)]
        revoke: bool,
    },
    /// The editor's language server, over standard input and output
    Lsp,
}

#[derive(Args, Debug)]
pub(super) struct RunArgs {
    /// Where to conjure: a file and a line, or a range of lines (src/cart.py:12, src/cart.py:12-20)
    #[arg(value_name = "FILE:LINE[-END]")]
    pub(super) target: Option<String>,
    /// Conjure for this scry finding
    #[arg(long, value_name = "ID")]
    pub(super) finding: Option<String>,
    /// Ask the stronger model, which may read the project first
    #[arg(long)]
    pub(super) try_harder: bool,
}
