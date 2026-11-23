//! The command line's grammar: the arguments and subcommands `scry` accepts.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "scry",
    version,
    about = "Reads a change and says what it gets wrong",
    args_conflicts_with_subcommands = true
)]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) cmd: Option<Cmd>,
    #[command(flatten)]
    pub(super) run: RunArgs,
}

#[derive(Args, Debug)]
pub(super) struct RunArgs {
    /// Files to review (their changed units; with --all, every unit). A file
    /// named like a subcommand is passed as ./list
    pub(super) files: Vec<PathBuf>,
    /// Every unit of each file in scope, not only the changed ones
    #[arg(long)]
    pub(super) all: bool,
    /// The files the branch changed since it left REV
    #[arg(long, value_name = "REV", conflicts_with = "files")]
    pub(super) since: Option<String>,
    /// The findings as JSON, one object a line
    #[arg(long)]
    pub(super) json: bool,
    /// Look again at units already reviewed as they are now
    #[arg(long)]
    pub(super) fresh: bool,
    /// The most this run may spend, in dollars (the day's cap still holds)
    #[arg(long, value_name = "USD")]
    pub(super) budget: Option<f64>,
    /// Stream the run log to stderr as the run goes
    #[arg(short, long)]
    pub(super) verbose: bool,
}

#[derive(Subcommand, Debug)]
pub(super) enum Cmd {
    /// The findings in the store, by status
    List {
        #[arg(long)]
        status: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// What scry spent, a local day a row
    Cost {
        #[arg(long, value_name = "YYYY-MM-DD")]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Stop showing a finding, here or everywhere
    Dismiss {
        id: String,
        #[arg(long)]
        everywhere: bool,
    },
    /// A finding in full
    Why { id: String },
    /// A run's log: the latest, or the Nth latest
    Log {
        /// 1 is the latest run, 2 the one before, …
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        n: Option<u32>,
        /// The logs kept, newest first: file, time, result
        #[arg(long, conflicts_with_all = ["n", "path"])]
        list: bool,
        /// The log's path only
        #[arg(long)]
        path: bool,
    },
    /// Whether scry can run here: provider, Jev, languages, steering, store, ledger
    Doctor,
    /// The editor's language server, over standard input and output
    Lsp,
    /// Let the editor review this project although its config runs code or moves your key
    Trust {
        /// Take the project off the trust list instead
        #[arg(long)]
        revoke: bool,
    },
}
