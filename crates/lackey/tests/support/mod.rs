//! Helpers shared by lackey's integration tests: a project in a temporary directory, a
//! scripted agent that calls back through the real loopback port, an editor that records, and a
//! clock that stands still.
#![allow(dead_code)]

mod fixed_clock;
mod grammars;
mod lsp_editor;
mod model_doubles;
mod recording_editor;
mod scripted_agent;
mod temp_project;

#[allow(unused_imports)]
pub use fixed_clock::*;
#[allow(unused_imports)]
pub use grammars::*;
#[allow(unused_imports)]
pub use lsp_editor::*;
#[allow(unused_imports)]
pub use model_doubles::*;
#[allow(unused_imports)]
pub use recording_editor::*;
#[allow(unused_imports)]
pub use scripted_agent::*;
#[allow(unused_imports)]
pub use temp_project::*;
