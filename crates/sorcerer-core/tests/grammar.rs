#![allow(clippy::too_many_lines)]
mod support;

use std::path::Path;

use sorcerer_core::Error;
use sorcerer_core::adapters::env::SystemEnv;
use sorcerer_core::adapters::grammar::{self, GrammarCache};
use sorcerer_core::adapters::platform::compiler::{CCompiler, c_compiler};
use sorcerer_core::domain::language::GrammarSource;

fn local(name: &str) -> GrammarSource {
    GrammarSource::Local(support::vendored(name))
}

fn compiler() -> Option<CCompiler> {
    c_compiler(&SystemEnv)
}

#[test]
fn a_cache_under_a_spaced_non_ascii_directory_builds_and_loads() {
    let d = tempfile::TempDir::new().unwrap();
    let cache = GrammarCache::at(d.path().join("a b é"));
    let lang = grammar::load(&cache, "python", &local("python")).unwrap();
    let mut p = tree_sitter::Parser::new();
    p.set_language(&lang).unwrap();
    let tree = p.parse("x = 1\n", None).unwrap();
    assert_eq!(tree.root_node().kind(), "module");
    assert!(!tree.root_node().has_error());
}

#[test]
fn builds_and_loads_the_vendored_python_grammar() {
    let d = tempfile::TempDir::new().unwrap();
    let lang = grammar::load(
        &GrammarCache::at(d.path().into()),
        "python",
        &local("python"),
    )
    .unwrap();
    let abi = lang.abi_version();
    assert!(
        (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION)
            .contains(&abi)
    );
    let mut p = tree_sitter::Parser::new();
    p.set_language(&lang).unwrap();
    let tree = p.parse("def f():\n    pass\n", None).unwrap();
    assert_eq!(tree.root_node().kind(), "module");
    assert!(!tree.root_node().has_error());
}

#[test]
fn the_rust_grammar_loads_too() {
    let lang = grammar::load(&support::test_cache(), "rust", &local("rust")).unwrap();
    let mut p = tree_sitter::Parser::new();
    p.set_language(&lang).unwrap();
    assert_eq!(
        p.parse("fn main() {}\n", None).unwrap().root_node().kind(),
        "source_file"
    );
}

#[test]
fn a_second_build_reuses_the_library() {
    let d = tempfile::TempDir::new().unwrap();
    let cache = GrammarCache::at(d.path().into());
    let first =
        grammar::build_if_absent(&cache, "python", &local("python"), compiler().as_ref()).unwrap();
    let t = std::fs::metadata(&first).unwrap().modified().unwrap();
    let second =
        grammar::build_if_absent(&cache, "python", &local("python"), compiler().as_ref()).unwrap();
    assert_eq!(first, second);
    assert_eq!(std::fs::metadata(&second).unwrap().modified().unwrap(), t);
}

#[test]
fn the_key_changes_with_the_rev() {
    let cache = GrammarCache::at("/c".into());
    let src = |rev: &str| GrammarSource::Git {
        url: "https://example.com/g".into(),
        rev: rev.into(),
    };
    let a = grammar::library_path(&cache, "g", &src("aaaaaaa")).unwrap();
    let b = grammar::library_path(&cache, "g", &src("bbbbbbb")).unwrap();
    assert_ne!(a, b);
    assert!(a.starts_with("/c") && a.file_name().unwrap().to_string_lossy().starts_with("g-"));
}

#[test]
fn no_compiler_is_a_stop_with_a_hint() {
    let d = tempfile::TempDir::new().unwrap();
    let e = grammar::build_if_absent(
        &GrammarCache::at(d.path().into()),
        "python",
        &local("python"),
        None,
    )
    .unwrap_err();
    assert!(matches!(e, Error::NoCompiler { .. }), "{e}");
    assert!(e.to_string().contains("cc, gcc or clang"), "{e}");
    assert!(e.to_string().contains("install"), "{e}");
}

#[test]
fn eight_threads_building_at_once_leave_one_library() {
    let d = tempfile::TempDir::new().unwrap();
    let cache = GrammarCache::at(d.path().into());
    std::thread::scope(|s| {
        let hs: Vec<_> = (0..8)
            .map(|_| {
                s.spawn(|| {
                    grammar::build_if_absent(
                        &cache,
                        "python",
                        &local("python"),
                        compiler().as_ref(),
                    )
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap().unwrap();
        }
    });
    let names: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    // and loads from many threads at once all succeed
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| grammar::load(&cache, "python", &local("python")).unwrap());
        }
    });
}

#[test]
fn a_git_grammar_is_fetched_at_its_rev() {
    // a local repository standing in for GitHub: file:// is one of git's own transports
    let repo = support::git_project();
    support::write(
        repo.path(),
        "src/parser.c",
        &std::fs::read_to_string(support::vendored("python").join("parser.c")).unwrap(),
    );
    support::write(
        repo.path(),
        "src/scanner.c",
        &std::fs::read_to_string(support::vendored("python").join("scanner.c")).unwrap(),
    );
    for h in ["alloc.h", "array.h", "parser.h"] {
        support::write(
            repo.path(),
            &format!("src/tree_sitter/{h}"),
            &std::fs::read_to_string(support::vendored("python").join("tree_sitter").join(h))
                .unwrap(),
        );
    }
    support::commit(repo.path(), "grammar");
    let rev = support::git(repo.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    let d = tempfile::TempDir::new().unwrap();
    let src = GrammarSource::Git {
        url: format!("file://{}", repo.path().display()),
        rev,
    };
    let lang = grammar::load(&GrammarCache::at(d.path().into()), "python", &src).unwrap();
    assert!(lang.node_kind_count() > 0);
    assert!(d.path().join("src").read_dir().unwrap().count() == 1);
}

#[test]
fn a_url_outside_gits_transports_is_refused() {
    let d = tempfile::TempDir::new().unwrap();
    let src = GrammarSource::Git {
        url: "ext::sh -c touch% /tmp/pwned".into(),
        rev: "a".repeat(40),
    };
    let e = grammar::load(&GrammarCache::at(d.path().into()), "x", &src).unwrap_err();
    assert!(matches!(e, Error::GrammarFetch { .. }), "{e}");
    assert!(!Path::new("/tmp/pwned").exists());
}

#[test]
fn a_failed_fetch_is_tried_again_on_the_next_load() {
    let repo = support::git_project();
    for f in [
        "parser.c",
        "scanner.c",
        "tree_sitter/alloc.h",
        "tree_sitter/array.h",
        "tree_sitter/parser.h",
    ] {
        support::write(
            repo.path(),
            &format!("src/{f}"),
            &std::fs::read_to_string(support::vendored("python").join(f)).unwrap(),
        );
    }
    support::commit(repo.path(), "grammar");
    let rev = support::git(repo.path(), &["rev-parse", "HEAD"])
        .trim()
        .to_string();
    let away = repo.path().with_extension("away");
    std::fs::rename(repo.path(), &away).unwrap();
    let d = tempfile::TempDir::new().unwrap();
    let cache = GrammarCache::at(d.path().into());
    let src = GrammarSource::Git {
        url: format!("file://{}", repo.path().display()),
        rev,
    };
    assert!(matches!(
        grammar::load(&cache, "python", &src),
        Err(Error::GrammarFetch { .. })
    ));
    std::fs::rename(&away, repo.path()).unwrap();
    assert!(grammar::load(&cache, "python", &src).is_ok());
}
