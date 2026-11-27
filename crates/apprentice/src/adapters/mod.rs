//! What implements the ports: the agent on `claude`, the loopback port and the helper
//! processes it serves (`apprentice mcp`, `apprentice hook`), the stores, the project's
//! context, the editors (the disk's, for the command line), the wiring.

// spellbook:region adapters-modules (from crates/apprentice/src/adapters/*.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod claude_agent;
pub mod disk_editor;
pub mod git_session;
pub mod grammar_syntax;
pub mod helper_way;
pub mod hooks;
pub mod json_permissions;
pub mod json_region_store;
pub mod jsonl_task_store;
pub mod loopback;
pub mod markdown_journal;
pub mod mcp_server;
pub mod project_context;
pub mod scry_files;
pub mod state_files;
pub mod steering_files;
pub mod system_clock;
pub mod task_runner;
pub mod wiring;
// spellbook:end adapters-modules
// spellbook:region adapters-folders (from crates/apprentice/src/adapters/*/mod.rs; edit .sorcerer/spellbook/regions.toml, not this block)
pub mod lsp;
// spellbook:end adapters-folders
