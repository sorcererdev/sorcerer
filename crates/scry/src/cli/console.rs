//! Where a command writes, and how wide the terminal is.

use std::io::{IsTerminal, Write};

/// The terminal's width in columns; 100 when stdout is not a terminal, or
/// its width cannot be read.
pub(super) fn terminal_width() -> usize {
    let terminal = std::io::stdout()
        .is_terminal()
        .then(terminal_size::terminal_size);
    terminal
        .flatten()
        .map_or(100, |(width, _)| usize::from(width.0))
}

/// Where a command writes: its output (`out`), its messages and errors
/// (`err`), and how many columns wide the terminal is.
pub(super) struct Console<'a> {
    pub(super) out: &'a mut dyn Write,
    pub(super) err: &'a mut dyn Write,
    pub(super) width: usize,
}

/// When the arguments do not parse: `--help` and `--version` print on
/// stdout and exit 0; a mistake prints on stderr and exits 2.
pub(super) fn print_usage_or_error(e: &clap::Error, console: &mut Console) -> i32 {
    let text = e.render().to_string();
    if e.use_stderr() {
        let _ = console.err.write_all(text.as_bytes());
        2
    } else {
        let _ = console.out.write_all(text.as_bytes());
        0
    }
}
