//! What a command says when its arguments do not parse.

use std::io::Write;

/// When the arguments do not parse: `--help` and `--version` print on stdout and exit 0; a
/// mistake prints on stderr and exits 2.
pub(super) fn print_usage_or_error(
    e: &clap::Error,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let text = e.render().to_string();
    if e.use_stderr() {
        let _ = err.write_all(text.as_bytes());
        2
    } else {
        let _ = out.write_all(text.as_bytes());
        0
    }
}
