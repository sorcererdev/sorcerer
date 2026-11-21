//! The real things behind the ports: the findings store as a JSON file, the
//! run log as Markdown files, the settings read from `.sorcerer/`, the
//! reviewer as a research call, the system clock, and the wiring that
//! builds them all for a project; and the editor's language server, which
//! calls the same use cases the command line calls.

pub mod json_finding_store;
pub mod lsp;
pub mod markdown_run_log;
pub mod research_reviewer;
pub mod scry_toml;
pub mod system_clock;
pub mod why_text;
pub mod wiring;
