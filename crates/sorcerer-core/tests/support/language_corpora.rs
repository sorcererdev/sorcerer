//! The language corpora, `corpus/languages/<name>/`: one tiny project per shipped language that
//! the per-language harnesses run on (`tests/languages.rs` in core, spellbook, lackey and
//! conjure). A harness reads only data here, so adding a language adds no harness code.
//!
//! Core's tests use this file as a module; spellbook, lackey and conjure include the same file
//! with `#[path = "../../../sorcerer-core/tests/support/language_corpora.rs"]`.
//!
//! `LANGUAGES_BLESS=1` makes every golden comparison write the file instead, for a person to
//! read line by line before committing it.

use std::path::{Path, PathBuf};

/// `corpus/languages/`.
pub fn language_corpora() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/languages")
}

/// The shipped languages, by the folders under `crates/sorcerer-core/library/languages/`, in
/// name order: every one has a corpus.
pub fn shipped_language_names() -> Vec<String> {
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../sorcerer-core/library/languages");
    let mut names: Vec<String> = std::fs::read_dir(&library)
        .unwrap_or_else(|e| panic!("{}: {e}", library.display()))
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// One language's corpus folder.
pub fn language_corpus(name: &str) -> PathBuf {
    language_corpora().join(name)
}

/// The files of a corpus's `base/`, by their path with `/`, in path order, with their text.
pub fn base_files(name: &str) -> Vec<(String, String)> {
    fn walk(root: &Path, dir: &Path, found: &mut Vec<(String, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, found);
            } else if let Ok(text) = std::fs::read_to_string(&path) {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push((rel, text.replace("\r\n", "\n")));
            }
        }
    }
    let base = language_corpus(name).join("base");
    let mut found = Vec::new();
    walk(&base, &base, &mut found);
    found.sort();
    found
}

/// Whether the goldens are being written rather than compared.
pub fn blessing_languages() -> bool {
    std::env::var_os("LANGUAGES_BLESS").is_some()
}

/// Compares `got` with the golden file, or writes it when blessing. A missing golden names the
/// file and how to write it.
pub fn matches_golden(golden: &Path, got: &str) -> Result<(), String> {
    if blessing_languages() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(golden, got).unwrap();
        return Ok(());
    }
    let expected = std::fs::read_to_string(golden).map_err(|e| {
        format!(
            "{}: {e} (write it with LANGUAGES_BLESS=1 and read it line by line)",
            golden.display()
        )
    })?;
    if expected.replace("\r\n", "\n") == got {
        Ok(())
    } else {
        Err(format!(
            "{} differs:\n--- expected\n{expected}--- got\n{got}",
            golden.display()
        ))
    }
}

/// Panics listing every problem found, one per line, when there is any: a harness checks every
/// language and reports them all at once.
pub fn assert_no_problems(what: &str, problems: &[String]) {
    assert!(
        problems.is_empty(),
        "{what}:\n{}",
        problems
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Applies a patch to a folder with `git apply`, as the corpus cases do.
pub fn apply_patch(root: &Path, patch: &Path) {
    let status = std::process::Command::new("git")
        .args(["apply", "--whitespace=nowarn"])
        .arg(patch)
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success(), "git apply {}", patch.display());
}

/// Copies a folder and everything in it (a corpus's `base/` into a test's project).
pub fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}
