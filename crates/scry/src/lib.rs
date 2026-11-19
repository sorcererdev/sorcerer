//! Scry: reads a change and says what it gets wrong. Config, issue types, steering
//! and the finding store live here; the pass is `pass`;
//! `live` wires it to the models and `cli` is the command line.

pub mod cli;
pub mod config;
pub mod finding;
pub mod issue_type;
pub mod judge;
pub mod live;
pub mod log;
pub mod pass;
pub mod review;
pub mod rounds;
pub mod scope;
pub mod steering;
pub mod store;
