//! `SystemEnv` reads the real environment; `MapEnv` is a fixed one for tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::adapters::platform::dirs::user_dir_vars;
use crate::ports::env::Env;

/// The real environment.
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.is_empty())
    }

    fn which(&self, exe: &str) -> Option<PathBuf> {
        which::which(exe).ok()
    }
}

/// A fixed environment: these variables, these executables on PATH.
#[derive(Default, Clone)]
pub struct MapEnv {
    pub vars: HashMap<String, String>,
    pub on_path: Vec<String>,
}

impl MapEnv {
    /// This environment with one more variable set.
    pub fn var_set(mut self, key: &str, value: &str) -> MapEnv {
        self.vars.insert(key.to_string(), value.to_string());
        self
    }

    /// This environment with one more executable on PATH (at `/fake/bin/<name>`).
    pub fn on_path(mut self, exe: &str) -> MapEnv {
        self.on_path.push(exe.to_string());
        self
    }

    /// This environment with every user directory (cache, config) under `home`, whatever the
    /// platform calls them. Example: on Linux, `with_user_dirs("/t")` sets `HOME=/t`,
    /// `XDG_CONFIG_HOME=/t/.config` and `XDG_CACHE_HOME=/t/.cache`.
    pub fn with_user_dirs(self, home: &Path) -> MapEnv {
        user_dir_vars(home)
            .into_iter()
            .fold(self, |env, (key, value)| env.var_set(key, &value))
    }
}

impl Env for MapEnv {
    fn var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn which(&self, exe: &str) -> Option<PathBuf> {
        self.on_path
            .iter()
            .any(|e| e == exe)
            .then(|| PathBuf::from("/fake/bin").join(exe))
    }
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_env_answers_vars_and_path() {
        let e = MapEnv::default().var_set("K", "v").on_path("claude");
        assert_eq!(e.var("K").as_deref(), Some("v"));
        assert!(e.var("X").is_none());
        assert!(e.which("claude").is_some());
        assert!(e.which("nope").is_none());
    }
}
