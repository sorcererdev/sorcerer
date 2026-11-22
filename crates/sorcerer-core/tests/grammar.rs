#![allow(clippy::too_many_lines)]
mod support;

use std::path::Path;

use sorcerer_core::Error;
use sorcerer_core::adapters::env::SystemEnv;
use sorcerer_core::adapters::grammar::{self, GrammarCache};
use sorcerer_core::adapters::platform::compiler::{CCompiler, c_compiler};
use sorcerer_core::domain::language::{GrammarObtained, GrammarOrigin, GrammarSource};

/// The origin the older tests load with: a grammar that is never downloaded.
const PROJECT: GrammarOrigin = GrammarOrigin::Project;

fn local(name: &str) -> GrammarSource {
    GrammarSource::Local(support::vendored(name))
}

fn compiler() -> Option<CCompiler> {
    c_compiler(&SystemEnv)
}

/// Copies a directory tree, files and folders, to a new place.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[test]
fn a_cache_under_a_spaced_non_ascii_directory_builds_and_loads() {
    // both the sources and the cache under the directory, as a fetched grammar's sources
    // are: the compiler's `-I`, input and output arguments all carry the space and the `é`
    let d = tempfile::TempDir::new().unwrap();
    let spaced = d.path().join("a b é");
    let sources = spaced.join("src").join("python");
    copy_dir(&support::vendored("python"), &sources);
    let cache = GrammarCache::at(spaced.join("cache"));
    let lang = grammar::load(&cache, "python", &GrammarSource::Local(sources), PROJECT)
        .unwrap()
        .0;
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
        PROJECT,
    )
    .unwrap()
    .0;
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
    let lang = grammar::load(&support::test_cache(), "rust", &local("rust"), PROJECT)
        .unwrap()
        .0;
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
    let d = tempfile::TempDir::new().unwrap();
    let cache = GrammarCache::at(d.path().into());
    let src = |rev: &str| GrammarSource::Git {
        url: "https://example.com/g".into(),
        rev: rev.into(),
    };
    let a = grammar::library_path(&cache, "g", &src("aaaaaaa")).unwrap();
    let b = grammar::library_path(&cache, "g", &src("bbbbbbb")).unwrap();
    assert_ne!(a, b);
    assert!(a.starts_with(d.path()) && a.file_name().unwrap().to_string_lossy().starts_with("g-"));
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
            s.spawn(|| grammar::load(&cache, "python", &local("python"), PROJECT).unwrap());
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
        url: support::file_url(repo.path()),
        rev,
    };
    let lang = grammar::load(&GrammarCache::at(d.path().into()), "python", &src, PROJECT)
        .unwrap()
        .0;
    assert!(lang.node_kind_count() > 0);
    assert!(d.path().join("src").read_dir().unwrap().count() == 1);
}

#[test]
fn a_url_outside_gits_transports_is_refused() {
    let d = tempfile::TempDir::new().unwrap();
    let pwned = d.path().join("pwned");
    let src = GrammarSource::Git {
        url: format!("ext::sh -c touch% {}", pwned.display()),
        rev: "a".repeat(40),
    };
    let e = grammar::load(
        &GrammarCache::at(d.path().join("cache")),
        "x",
        &src,
        PROJECT,
    )
    .unwrap_err();
    assert!(matches!(e, Error::GrammarFetch { .. }), "{e}");
    assert!(!pwned.exists());
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
        url: support::file_url(repo.path()),
        rev,
    };
    assert!(matches!(
        grammar::load(&cache, "python", &src, PROJECT),
        Err(Error::GrammarFetch { .. })
    ));
    std::fs::rename(&away, repo.path()).unwrap();
    assert!(grammar::load(&cache, "python", &src, PROJECT).is_ok());
}

// ---- a shipped grammar downloaded prebuilt from scry's release ----

/// A local repository holding the vendored python grammar, standing in for GitHub, and the
/// shipped-style source that names it at its one commit. Fetching from it needs no network.
fn python_repo() -> (tempfile::TempDir, GrammarSource) {
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
    let source = GrammarSource::Git {
        url: support::file_url(repo.path()),
        rev,
    };
    (repo, source)
}

/// The vendored python grammar compiled once for this machine: what a release would hold.
fn prebuilt_python() -> Vec<u8> {
    let d = tempfile::TempDir::new().unwrap();
    let lib = grammar::build_if_absent(
        &GrammarCache::at(d.path().into()),
        "python",
        &local("python"),
        compiler().as_ref(),
    )
    .unwrap();
    std::fs::read(lib).unwrap()
}

/// `<hex>  <file name>\n`, the way a release's `.sha256` file reads.
fn sha256_line(bytes: &[u8], file: &str) -> String {
    use sha2::Digest;
    let hex: String = sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{hex}  {file}\n")
}

/// The file name the cache asks the release for.
fn release_file(cache: &GrammarCache, source: &GrammarSource) -> String {
    grammar::library_path(cache, "python", source)
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// The names in the cache directory that are temporary files.
fn temporary_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .filter(|name| name.contains(".tmp-"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn a_shipped_grammar_is_downloaded_and_loads() {
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let mut server = mockito::Server::new();
    let cache = GrammarCache::at(d.path().into()).with_releases(&server.url());
    let file = release_file(&cache, &source);
    let bytes = prebuilt_python();
    let library = server
        .mock("GET", format!("/{file}").as_str())
        .with_body(&bytes)
        .expect(1)
        .create();
    let sum = server
        .mock("GET", format!("/{file}.sha256").as_str())
        .with_body(sha256_line(&bytes, &file))
        .expect(1)
        .create();

    let (lang, obtained) =
        grammar::load(&cache, "python", &source, GrammarOrigin::Shipped).unwrap();
    assert_eq!(obtained, GrammarObtained::Downloaded);
    assert!(lang.node_kind_count() > 0);
    assert_eq!(std::fs::read(d.path().join(&file)).unwrap(), bytes);
    // nothing was fetched with git: the release's file was enough
    assert!(!d.path().join("src").exists());
    library.assert();
    sum.assert();

    let (_, again) = grammar::load(&cache, "python", &source, GrammarOrigin::Shipped).unwrap();
    assert_eq!(again, GrammarObtained::Cached);
}

#[test]
fn a_404_falls_back_to_compiling() {
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let mut server = mockito::Server::new();
    let cache = GrammarCache::at(d.path().into()).with_releases(&server.url());
    let file = release_file(&cache, &source);
    let missing = server
        .mock("GET", format!("/{file}").as_str())
        .with_status(404)
        .with_body("Not Found")
        .create();

    let (_, obtained) = grammar::load(&cache, "python", &source, GrammarOrigin::Shipped).unwrap();
    assert!(
        matches!(obtained, GrammarObtained::Built { .. }),
        "{obtained:?}"
    );
    assert!(temporary_files(d.path()).is_empty());
    missing.assert();
}

#[test]
fn a_bad_sum_deletes_the_file_and_compiles() {
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let mut server = mockito::Server::new();
    let cache = GrammarCache::at(d.path().into()).with_releases(&server.url());
    let file = release_file(&cache, &source);
    // an HTML page where the library should be, with the real library's sum
    let page = b"<html><body>Sign in to continue</body></html>".to_vec();
    server
        .mock("GET", format!("/{file}").as_str())
        .with_body(&page)
        .create();
    server
        .mock("GET", format!("/{file}.sha256").as_str())
        .with_body(sha256_line(&prebuilt_python(), &file))
        .create();

    let (lang, obtained) =
        grammar::load(&cache, "python", &source, GrammarOrigin::Shipped).unwrap();
    assert!(
        matches!(obtained, GrammarObtained::Built { .. }),
        "{obtained:?}"
    );
    assert!(lang.node_kind_count() > 0);
    assert!(
        temporary_files(d.path()).is_empty(),
        "{:?}",
        temporary_files(d.path())
    );
    assert_ne!(std::fs::read(d.path().join(&file)).unwrap(), page);
}

#[test]
fn a_body_past_the_size_cap_is_refused() {
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let mut server = mockito::Server::new();
    let cache = GrammarCache::at(d.path().into()).with_releases(&server.url());
    let file = release_file(&cache, &source);
    // one byte past 20 MB, with its own correct sum: only the size refuses it
    let huge = vec![0u8; 20 * 1024 * 1024 + 1];
    server
        .mock("GET", format!("/{file}").as_str())
        .with_body(&huge)
        .create();
    server
        .mock("GET", format!("/{file}.sha256").as_str())
        .with_body(sha256_line(&huge, &file))
        .create();

    let (_, obtained) = grammar::load(&cache, "python", &source, GrammarOrigin::Shipped).unwrap();
    assert!(
        matches!(obtained, GrammarObtained::Built { .. }),
        "{obtained:?}"
    );
    assert!(temporary_files(d.path()).is_empty());
    assert_ne!(
        std::fs::metadata(d.path().join(&file)).unwrap().len(),
        huge.len() as u64
    );
}

#[test]
fn a_project_grammar_never_asks() {
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let mut server = mockito::Server::new();
    let anything = server.mock("GET", mockito::Matcher::Any).expect(0).create();
    let cache = GrammarCache::at(d.path().into()).with_releases(&server.url());

    let (_, obtained) = grammar::load(&cache, "python", &source, GrammarOrigin::Project).unwrap();
    assert!(
        matches!(obtained, GrammarObtained::Built { .. }),
        "{obtained:?}"
    );
    // nor does a shipped language whose grammar is a local directory
    let other = tempfile::TempDir::new().unwrap();
    let cache = GrammarCache::at(other.path().into()).with_releases(&server.url());
    grammar::load(&cache, "python", &local("python"), GrammarOrigin::Shipped).unwrap();
    anything.assert();
}

#[test]
fn a_cache_at_a_directory_never_downloads() {
    // no server at all: `GrammarCache::at` knows no release, so this builds as before
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let cache = GrammarCache::at(d.path().into());
    let (_, obtained) = grammar::load(&cache, "python", &source, GrammarOrigin::Shipped).unwrap();
    match obtained {
        GrammarObtained::Built { compiler } => {
            let want = c_compiler(&SystemEnv).unwrap().program;
            assert_eq!(
                Some(compiler.as_str()),
                want.file_stem().and_then(|s| s.to_str())
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_release_url_names_this_version() {
    assert_eq!(
        grammar::RELEASES,
        format!(
            "https://github.com/sorcererdev/sorcerer/releases/download/v{}",
            env!("CARGO_PKG_VERSION")
        )
    );
}

#[test]
fn a_downloaded_library_that_will_not_load_is_compiled_instead() {
    // the sum matches, but the file is no library for this machine (a release built wrong,
    // or for an older system than this one): it is deleted and the grammar compiled here
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let mut server = mockito::Server::new();
    let cache = GrammarCache::at(d.path().into()).with_releases(&server.url());
    let file = release_file(&cache, &source);
    let not_a_library = b"\x7fELF but not really".to_vec();
    server
        .mock("GET", format!("/{file}").as_str())
        .with_body(&not_a_library)
        .create();
    server
        .mock("GET", format!("/{file}.sha256").as_str())
        .with_body(sha256_line(&not_a_library, &file))
        .create();

    let (lang, obtained) =
        grammar::load(&cache, "python", &source, GrammarOrigin::Shipped).unwrap();
    assert!(
        matches!(obtained, GrammarObtained::Built { .. }),
        "{obtained:?}"
    );
    assert!(lang.node_kind_count() > 0);
    assert_ne!(std::fs::read(d.path().join(&file)).unwrap(), not_a_library);
}

#[test]
fn a_failed_download_without_a_compiler_says_why() {
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let mut server = mockito::Server::new();
    let cache = GrammarCache::at(d.path().into()).with_releases(&server.url());
    let file = release_file(&cache, &source);
    server
        .mock("GET", format!("/{file}").as_str())
        .with_status(404)
        .create();

    let e = grammar::load_with_compiler(&cache, "python", &source, GrammarOrigin::Shipped, None)
        .unwrap_err();
    let want = format!(
        "couldn't download the python grammar (no release for {}); install a C compiler: ",
        env!("CARGO_PKG_VERSION")
    );
    assert!(e.to_string().starts_with(&want), "{e}");
    assert!(
        e.to_string()
            .ends_with(sorcerer_core::adapters::platform::compiler::compiler_hint()),
        "{e}"
    );
    // nothing is fetched that could not be compiled
    assert!(!d.path().join("src").exists());
}

#[test]
fn a_sum_that_does_not_match_is_named_in_that_error() {
    let (_repo, source) = python_repo();
    let d = tempfile::TempDir::new().unwrap();
    let mut server = mockito::Server::new();
    let cache = GrammarCache::at(d.path().into()).with_releases(&server.url());
    let file = release_file(&cache, &source);
    server
        .mock("GET", format!("/{file}").as_str())
        .with_body("<html>a proxy's page</html>")
        .create();
    server
        .mock("GET", format!("/{file}.sha256").as_str())
        .with_body(sha256_line(b"something else", &file))
        .create();

    let e = grammar::load_with_compiler(&cache, "python", &source, GrammarOrigin::Shipped, None)
        .unwrap_err();
    assert!(
        e.to_string()
            .contains("(its SHA-256 did not match the release's)"),
        "{e}"
    );
}

#[test]
fn a_user_cache_with_no_place_refuses_and_names_what_to_set() {
    use sorcerer_core::adapters::env::MapEnv;
    use sorcerer_core::adapters::platform::dirs::NO_CACHE_DIR;
    // no HOME, no XDG_CACHE_HOME (no LOCALAPPDATA on Windows): no cache in the current
    // directory, where a repository could have put a library of its own
    let cache = GrammarCache::user(&MapEnv::default());
    let e = grammar::load(&cache, "python", &local("python"), PROJECT).unwrap_err();
    assert!(matches!(e, Error::NoCacheDir { .. }), "{e}");
    assert_eq!(
        e.to_string(),
        format!("there is no place for the grammar cache: {NO_CACHE_DIR}")
    );

    let home = tempfile::TempDir::new().unwrap();
    let cache = GrammarCache::user(&MapEnv::default().with_user_dirs(home.path()));
    assert!(cache.dir().unwrap().starts_with(home.path()));
}
