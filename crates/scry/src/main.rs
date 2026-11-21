use std::io::Write;

use sorcerer_core::adapters::env::SystemEnv;

fn main() {
    // not locked for the whole run: `scry lsp` writes the protocol from a thread of its own
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    let code = scry::cli::main(
        std::env::args_os().collect(),
        &SystemEnv,
        &mut out,
        &mut err,
    );
    let _ = out.flush();
    std::process::exit(code);
}
