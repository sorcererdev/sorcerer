//! The command line's grammar: the arguments and subcommands `spellbook` accepts.

use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "spellbook",
    version,
    about = "A project's conventions as data: generators with holes"
)]
pub(super) struct Cli {}
