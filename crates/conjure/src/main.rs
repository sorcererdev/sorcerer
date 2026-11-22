use std::io::Write;

use sorcerer_core::adapters::env::SystemEnv;

fn main() {
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    let code = conjure::cli::main(
        std::env::args_os().collect(),
        &SystemEnv,
        &mut out,
        &mut err,
    );
    let _ = out.flush();
    std::process::exit(code);
}
