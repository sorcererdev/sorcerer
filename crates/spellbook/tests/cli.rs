//! The command line, on a copy of bistro with the vendored grammars.

use std::ffi::OsString;

use sorcerer_core::adapters::env::MapEnv;
use spellbook::cli::main_with;

fn args(rest: &[&str]) -> Vec<OsString> {
    std::iter::once("spellbook")
        .chain(rest.iter().copied())
        .map(OsString::from)
        .collect()
}

#[test]
fn version_prints_the_name() {
    let dir = tempfile::tempdir().unwrap();
    let env = MapEnv::default().with_user_dirs(dir.path());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = main_with(args(&["--version"]), dir.path(), &env, &mut out, &mut err);
    assert_eq!(code, 0);
    assert!(String::from_utf8(out).unwrap().starts_with("spellbook "));
}
