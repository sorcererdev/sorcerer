//! The user's own directories: where sorcerer caches what it builds and keeps what the user
//! told it.
//!
//! | | Unix (Linux, macOS) | Windows |
//! |---|---|---|
//! | cache | `$XDG_CACHE_HOME`, else `$HOME/.cache` | `%LOCALAPPDATA%` |
//! | config | `$XDG_CONFIG_HOME`, else `$HOME/.config` | `%APPDATA%` |

use std::path::{Path, PathBuf};

use crate::ports::env::Env;

/// Why there is no config directory, for a message that names what to set.
#[cfg(not(windows))]
pub const NO_CONFIG_DIR: &str = "neither XDG_CONFIG_HOME nor HOME is set";
/// Why there is no config directory, for a message that names what to set.
#[cfg(windows)]
pub const NO_CONFIG_DIR: &str = "APPDATA is not set";

/// The directory for files that can be rebuilt, such as compiled grammars; `None` when the
/// environment names none. Example: on Linux with only `HOME=/home/ann` set, it is
/// `/home/ann/.cache`.
pub fn user_cache_dir(env: &dyn Env) -> Option<PathBuf> {
    if cfg!(windows) {
        env.var("LOCALAPPDATA").map(PathBuf::from)
    } else {
        xdg_or_home(env, "XDG_CACHE_HOME", ".cache")
    }
}

/// The directory for the user's settings, such as the trusted projects; `None` when the
/// environment names none. Example: on Windows with `APPDATA=C:\Users\ann\AppData\Roaming`,
/// it is that directory.
pub fn user_config_dir(env: &dyn Env) -> Option<PathBuf> {
    if cfg!(windows) {
        env.var("APPDATA").map(PathBuf::from)
    } else {
        xdg_or_home(env, "XDG_CONFIG_HOME", ".config")
    }
}

/// The XDG variable when it is set, else the named directory under `$HOME`.
fn xdg_or_home(env: &dyn Env, xdg: &str, under_home: &str) -> Option<PathBuf> {
    env.var(xdg).map(PathBuf::from).or_else(|| {
        env.var("HOME")
            .map(|home| PathBuf::from(home).join(under_home))
    })
}

/// The environment variables that put every user directory under `home`, for tests that run
/// sorcerer as a fake user. On Unix they are `HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME`;
/// on Windows `USERPROFILE`, `APPDATA` and `LOCALAPPDATA`.
#[doc(hidden)]
pub fn user_dir_vars(home: &Path) -> Vec<(&'static str, String)> {
    let text = |path: PathBuf| path.to_string_lossy().into_owned();
    if cfg!(windows) {
        vec![
            ("USERPROFILE", text(home.to_path_buf())),
            ("APPDATA", text(home.join("AppData").join("Roaming"))),
            ("LOCALAPPDATA", text(home.join("AppData").join("Local"))),
        ]
    } else {
        vec![
            ("HOME", text(home.to_path_buf())),
            ("XDG_CONFIG_HOME", text(home.join(".config"))),
            ("XDG_CACHE_HOME", text(home.join(".cache"))),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::env::MapEnv;

    #[cfg(not(windows))]
    #[test]
    fn the_cache_follows_xdg_then_home() {
        let xdg = MapEnv::default()
            .var_set("XDG_CACHE_HOME", "/x")
            .var_set("HOME", "/h");
        assert_eq!(user_cache_dir(&xdg), Some(PathBuf::from("/x")));
        let home = MapEnv::default().var_set("HOME", "/h");
        assert_eq!(user_cache_dir(&home), Some(PathBuf::from("/h/.cache")));
        assert_eq!(user_cache_dir(&MapEnv::default()), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_config_follows_xdg_then_home() {
        let xdg = MapEnv::default()
            .var_set("XDG_CONFIG_HOME", "/x")
            .var_set("HOME", "/h");
        assert_eq!(user_config_dir(&xdg), Some(PathBuf::from("/x")));
        let home = MapEnv::default().var_set("HOME", "/h");
        assert_eq!(user_config_dir(&home), Some(PathBuf::from("/h/.config")));
        assert_eq!(user_config_dir(&MapEnv::default()), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_cache_is_local_app_data() {
        let env = MapEnv::default()
            .var_set("LOCALAPPDATA", r"C:\Users\ann\AppData\Local")
            .var_set("APPDATA", r"C:\Users\ann\AppData\Roaming")
            .var_set("HOME", r"C:\elsewhere");
        assert_eq!(
            user_cache_dir(&env),
            Some(PathBuf::from(r"C:\Users\ann\AppData\Local"))
        );
        assert_eq!(
            user_config_dir(&env),
            Some(PathBuf::from(r"C:\Users\ann\AppData\Roaming"))
        );
        assert_eq!(user_config_dir(&MapEnv::default()), None);
    }

    #[test]
    fn the_user_dir_vars_name_the_directories_the_lookups_read() {
        let home = Path::new("fake-home");
        let env = MapEnv::default().with_user_dirs(home);
        assert!(user_cache_dir(&env).unwrap().starts_with(home));
        assert!(user_config_dir(&env).unwrap().starts_with(home));
    }
}
