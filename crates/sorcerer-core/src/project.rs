//! The project: its root, its config and its state directory. Opened from
//! any directory inside it; out of the box (no `.sorcerer/`) it is the git
//! root with the default config.

use std::path::{Path, PathBuf};

use crate::config::{Config, DecideConfig, ModelConfig};
use crate::env::Env;
use crate::language::Languages;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct Project {
    /// The directory holding `.sorcerer/`, or the git root when there is none.
    pub root: PathBuf,
    pub config: Config,
    /// The shipped and project language layers, merged.
    pub languages: Languages,
    /// `.sorcerer/state`, created on first write ([`Project::state_dir`]).
    pub state: PathBuf,
}

impl Project {
    /// Walks up from DIR: the first directory holding `.sorcerer/` is the
    /// root and its `config.toml` (if any) is read; else the first holding
    /// `.git` is the root with the default config.
    pub fn open(dir: &Path) -> Result<Project> {
        let dir = dir.canonicalize().map_err(|e| Error::io(dir, e))?;
        let root = find_up(&dir, |d| d.join(".sorcerer").is_dir())
            .or_else(|| find_up(&dir, |d| d.join(".git").exists()))
            .ok_or(Error::NoProject { from: dir.clone() })?;
        let file = root.join(".sorcerer/config.toml");
        let config = match std::fs::read_to_string(&file) {
            Ok(text) => Config::parse(&text, &file)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(Error::io(&file, e)),
        };
        let languages = Languages::assemble(&root, &config.languages)?;
        let state = root.join(".sorcerer/state");
        Ok(Project {
            root,
            config,
            languages,
            state,
        })
    }

    /// The state directory, created on first call with a `.gitignore` of
    /// `*` so nothing in it is ever committed.
    pub fn state_dir(&self) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.state).map_err(|e| Error::io(&self.state, e))?;
        let ignore = self.state.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n").map_err(|e| Error::io(&ignore, e))?;
        }
        Ok(self.state.clone())
    }
}

fn find_up(dir: &Path, found: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    dir.ancestors().find(|d| found(d)).map(Path::to_path_buf)
}

/// Which provider or decider was chosen, and why (for `doctor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderChoice {
    pub name: String,
    pub why: String,
}

fn choice(name: &str, why: &str) -> Option<ProviderChoice> {
    Some(ProviderChoice {
        name: name.to_string(),
        why: why.to_string(),
    })
}

/// The provider to use: `[model] provider` if set; else `claude` when it is
/// on PATH (a subscription); else `anthropic` when `ANTHROPIC_API_KEY` is
/// set; else none.
pub fn default_provider(cfg: &ModelConfig, env: &dyn Env) -> Option<ProviderChoice> {
    if let Some(p) = &cfg.provider {
        return choice(p, "set in [model]");
    }
    if env.which("claude").is_some() {
        return choice("claude", "claude is on PATH");
    }
    if env.var("ANTHROPIC_API_KEY").is_some() {
        return choice("anthropic", "ANTHROPIC_API_KEY is set");
    }
    None
}

/// The decider: Jev when its key (`[decide] key_env`, default `JEV_API_KEY`) is set.
pub fn default_decider(cfg: &DecideConfig, env: &dyn Env) -> Option<ProviderChoice> {
    let var = cfg.key_env.as_deref().unwrap_or("JEV_API_KEY");
    env.var(var)
        .and_then(|_| choice("jev", &format!("{var} is set")))
}
