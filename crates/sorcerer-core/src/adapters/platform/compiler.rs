//! The C compiler that turns a grammar's sources into a shared library, and what that library
//! is called on each platform.

use std::path::PathBuf;

use crate::ports::env::Env;

/// A C compiler on PATH and the flags that make it build a shared library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CCompiler {
    pub program: PathBuf,
    pub flags: &'static [&'static str],
}

/// The file extension of a shared library: `dylib` on macOS, `dll` on Windows, `so` elsewhere.
pub fn shared_library_extension() -> &'static str {
    if cfg!(target_os = "macos") {
        "dylib"
    } else if cfg!(windows) {
        "dll"
    } else {
        "so"
    }
}

/// The first of `cc`, `gcc` and `clang` found on PATH, with the flags for a shared library
/// (`-shared -fPIC -O2` on Unix; `-shared -O2` on Windows, where every library is
/// position-independent). `None` when none of the three is there.
pub fn c_compiler(env: &dyn Env) -> Option<CCompiler> {
    let flags: &'static [&'static str] = if cfg!(windows) {
        &["-shared", "-O2"]
    } else {
        &["-shared", "-fPIC", "-O2"]
    };
    ["cc", "gcc", "clang"]
        .into_iter()
        .find_map(|name| env.which(name))
        .map(|program| CCompiler { program, flags })
}

/// How to install a C compiler on this platform, in a few words a user can type or search.
pub fn compiler_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "xcode-select --install"
    } else if cfg!(windows) {
        "install MinGW-w64 (gcc) or LLVM (clang)"
    } else {
        "apt install gcc"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::env::MapEnv;

    #[test]
    fn the_first_compiler_on_path_wins() {
        let env = MapEnv::default().on_path("clang").on_path("gcc");
        let found = c_compiler(&env).unwrap();
        assert!(found.program.ends_with("gcc"), "{found:?}");
        assert!(found.flags.contains(&"-shared"));
        assert!(c_compiler(&MapEnv::default()).is_none());
    }
}
