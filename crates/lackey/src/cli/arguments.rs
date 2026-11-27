//! The command line's grammar: the arguments and subcommands `lackey` accepts.

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "lackey",
    version,
    about = "A coding agent whose whole interface is your editor",
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
    /// The editor's language server, over standard input and output
    Lsp,
    /// The medium tools as an MCP server (the agent starts this)
    Mcp,
    /// A hook of the agent's: post-tool-use or stop (the agent runs this)
    Hook { event: String },
    /// What lackey spent, a local day a row
    Cost {
        #[arg(long, value_name = "YYYY-MM-DD")]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// The last tasks, newest first
    Log {
        /// How many (10 by default)
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        n: Option<u32>,
    },
    /// Whether lackey can run here: claude, Jev, the config, trust, the records
    Doctor,
    /// Let the editor run lackey in this project although its config runs code or moves your key
    Trust {
        /// Take the project off the trust list instead
        #[arg(long)]
        revoke: bool,
    },
}

#[derive(Args, Debug)]
pub(super) struct RunArgs {
    /// Where the instruction is about: a file and a line (src/cart.py:12)
    #[arg(value_name = "FILE:LINE")]
    pub(super) target: Option<String>,
    /// What to do, in your words
    #[arg(value_name = "INSTRUCTION")]
    pub(super) instruction: Option<String>,
}
