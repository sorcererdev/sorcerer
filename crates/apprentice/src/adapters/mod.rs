//! What implements the ports: the agent on `claude`, the loopback port and the helper
//! processes it serves (`apprentice mcp`, `apprentice hook`), the stores, the project's
//! context, the editors (the disk's, for the command line), the wiring.

pub mod claude_agent;
pub mod disk_editor;
pub mod helper_way;
pub mod hooks;
pub mod json_permissions;
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
