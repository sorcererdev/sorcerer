//! Core's typed errors. Each variant's message is one sentence a user can act on.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {why}", file.display())]
    Config { file: PathBuf, why: String },
    #[error("unknown key `{key}` in [{table}]")]
    UnknownKey { table: String, key: String },
    #[error("no project at {}: no .sorcerer/ directory and no git repository above it", from.display())]
    NoProject { from: PathBuf },
    #[error("language {name}: {why}")]
    Language { name: String, why: String },
    #[error("grammar {name}: {why}")]
    GrammarFetch { name: String, why: String },
    #[error("no C compiler (cc) found; install one: {hint}")]
    NoCompiler { hint: String },
    #[error("grammar {name} did not compile: {stderr}")]
    GrammarBuild { name: String, stderr: String },
    #[error("grammar {name} could not be loaded: {why}")]
    GrammarLoad { name: String, why: String },
    #[error("grammar {name} has ABI {abi}; this sorcerer reads {min}–{max}")]
    GrammarAbi {
        name: String,
        abi: usize,
        min: usize,
        max: usize,
    },
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// An I/O error at PATH.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Error {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}
