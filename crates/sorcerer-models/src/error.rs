//! The models crate's errors. Each message is one sentence a user can act on.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] sorcerer_core::Error),
    #[error("{provider}: no key; set {var}")]
    NoKey { provider: String, var: String },
    #[error("{provider}: HTTP {status}: {body}")]
    Http {
        provider: String,
        status: u16,
        body: String,
    },
    #[error("{provider}: {why}")]
    Transport { provider: String, why: String },
    #[error("{provider}: timed out after {seconds} s")]
    Timeout { provider: String, seconds: u64 },
    #[error("cancelled")]
    Cancelled,
    #[error("{provider}: unknown model alias `{alias}`; {hint}")]
    UnknownAlias {
        provider: String,
        alias: String,
        hint: String,
    },
    #[error("{provider}: {why}")]
    Reply { provider: String, why: String },
    /// The provider stopped the call and said so after spending money on it (`claude -p` at its
    /// `--max-budget-usd`): the spend is kept so the ledger charges it.
    #[error("{provider}: {why}")]
    Spent {
        provider: String,
        why: String,
        cost_usd: Option<f64>,
    },
    #[error("{why}")]
    Setup { why: String },
    #[error("{runner}: {why}")]
    Subprocess { runner: String, why: String },
}

pub type Result<T> = std::result::Result<T, Error>;
