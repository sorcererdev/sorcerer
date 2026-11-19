//! The ports implemented: the store as a JSON file, the run log as Markdown
//! files, the settings from `.sorcerer/`, the reviewer over a researcher,
//! the system clock, and the wiring that builds them for a project.

pub mod json_finding_store;
pub mod markdown_run_log;
pub mod research_reviewer;
pub mod scry_toml;
pub mod system_clock;
pub mod wiring;
