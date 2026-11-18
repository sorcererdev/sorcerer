//! Core's typed errors. Each variant's message is one sentence a user can act on.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {why}", file.display())]
    Config { file: PathBuf, why: String },
    #[error("unknown key `{key}` in [{table}]")]
    UnknownKey { table: String, key: String },
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
