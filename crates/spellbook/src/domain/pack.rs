//! A pack: another repository's `.sorcerer/` (its generators, rules, regions and config), copied
//! into a project once by `spellbook init --pack`, which records where it came from in
//! `.sorcerer/pack.toml`.

use serde::Serialize;

/// Where the record of the installed pack is kept, relative to `.sorcerer/`.
pub const PACK_RECORD: &str = "pack.toml";

/// The folder a project keeps its spellbook in, relative to `.sorcerer/`: a project that has it
/// has conventions of its own, and a pack is not laid over them.
pub const SPELLBOOK_DIR: &str = "spellbook";

/// What `--pack` names: a repository (a git URL or a path to a local repository) and, after a
/// final `@`, the revision to take (a tag, a branch or a commit). No revision is the repository's
/// default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackAddress {
    pub repo: String,
    pub rev: Option<String>,
}

impl PackAddress {
    /// Reads `<repo>[@rev]`. The `@` that starts a revision is the last one, and only when no `/`,
    /// `\` or `:` follows it, so the `@` of `git@github.com:org/pack.git` is part of the address.
    ///
    /// Example: `https://github.com/org/pack@v1.2` is the repository `https://github.com/org/pack`
    /// at `v1.2`; `git@github.com:org/pack.git` has no revision.
    pub fn parse(text: &str) -> Result<PackAddress, String> {
        let text = text.trim();
        let (repo, rev) = match text.rsplit_once('@') {
            Some((repo, rev)) if !rev.contains(['/', '\\', ':']) => (repo, Some(rev)),
            _ => (text, None),
        };
        if repo.is_empty() || rev.is_some_and(str::is_empty) {
            return Err(format!(
                "`{text}` is not a pack address; write a git URL or a path, with @REV to pick a revision"
            ));
        }
        Ok(PackAddress {
            repo: repo.to_string(),
            rev: rev.map(str::to_string),
        })
    }
}

/// `.sorcerer/pack.toml`: which pack the project's `.sorcerer/` came from, so a later command can
/// show what the pack has changed since.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackRecord {
    pub name: String,
    pub repo: String,
    /// The commit the files were copied from (the revision asked for, resolved).
    pub rev: String,
}

impl PackRecord {
    /// The record as `pack.toml` writes it.
    ///
    /// Example: `name = "rust-web"`, `repo = "https://github.com/org/pack-rust-web"`,
    /// `rev = "1a2b…"`, one per line.
    pub fn text(&self) -> String {
        let table = toml::to_string(self).unwrap_or_default();
        format!(
            "# The pack this project's .sorcerer/ was copied from (spellbook init --pack).\n{table}"
        )
    }
}

/// The pack's name: `name` in the `pack.toml` at the root of its repository, else the
/// repository's last path part without `.git`.
///
/// Example: with no `pack.toml`, `https://github.com/org/pack-rust-web.git` is `pack-rust-web`.
pub fn pack_name(manifest: Option<&str>, repo: &str) -> String {
    let named = manifest
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("name")?.as_str().map(str::to_string))
        .filter(|name| !name.trim().is_empty());
    named.unwrap_or_else(|| repo_basename(repo))
}

fn repo_basename(repo: &str) -> String {
    let last = repo
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or(repo);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

/// Why a pack was not installed. Nothing was written, unless the disk failed part way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackStop {
    /// The project has `.sorcerer/spellbook/` already.
    HasSpellbook,
    /// `git` could not fetch the repository or the revision: its first line of complaint.
    Fetch(String),
    /// The repository has no `.sorcerer/` folder.
    NotAPack,
    /// The pack's `.sorcerer/` holds a link; only plain files are copied.
    Link(String),
    /// The disk refused a file.
    Disk(String),
}

/// The stop as one sentence.
///
/// Example: `the project has .sorcerer/spellbook/ already; a pack is copied only into a project
/// without one (nothing was written)`.
pub fn pack_stop_text(stop: &PackStop) -> String {
    match stop {
        PackStop::HasSpellbook => format!(
            "the project has .sorcerer/{SPELLBOOK_DIR}/ already; a pack is copied only into a project without one (nothing was written)"
        ),
        PackStop::Fetch(why) => format!("the pack could not be fetched: {why}"),
        PackStop::NotAPack => {
            "the repository has no .sorcerer/ folder, so it is not a pack (nothing was written)"
                .into()
        }
        PackStop::Link(path) => format!(
            "the pack's .sorcerer/{path} is a link; a pack holds plain files only (nothing was written)"
        ),
        PackStop::Disk(why) => why.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(repo: &str, rev: Option<&str>) -> PackAddress {
        PackAddress {
            repo: repo.into(),
            rev: rev.map(str::to_string),
        }
    }

    #[test]
    fn an_address_takes_a_revision_after_its_last_at() {
        let cases = [
            (
                "https://github.com/org/pack",
                address("https://github.com/org/pack", None),
            ),
            (
                "https://github.com/org/pack@v1.2",
                address("https://github.com/org/pack", Some("v1.2")),
            ),
            (
                "git@github.com:org/pack.git",
                address("git@github.com:org/pack.git", None),
            ),
            (
                "git@github.com:org/pack.git@abc123",
                address("git@github.com:org/pack.git", Some("abc123")),
            ),
            ("../packs/web@main", address("../packs/web", Some("main"))),
            ("C:\\packs\\web", address("C:\\packs\\web", None)),
        ];
        for (text, want) in cases {
            assert_eq!(PackAddress::parse(text), Ok(want), "{text}");
        }
        assert!(PackAddress::parse("").is_err());
        assert!(PackAddress::parse("../web@").is_err());
    }

    #[test]
    fn the_name_is_the_manifests_else_the_repositorys() {
        assert_eq!(pack_name(Some("name = \"rust-web\"\n"), "x/y"), "rust-web");
        assert_eq!(
            pack_name(None, "https://github.com/org/pack-rust-web.git"),
            "pack-rust-web"
        );
        assert_eq!(pack_name(Some("version = \"1\""), "/tmp/packs/web/"), "web");
    }

    #[test]
    fn the_record_is_toml_with_the_three_keys() {
        let record = PackRecord {
            name: "rust-web".into(),
            repo: "../web".into(),
            rev: "abc".into(),
        };
        let table: toml::Table = record.text().parse().unwrap();
        assert_eq!(table["name"].as_str(), Some("rust-web"));
        assert_eq!(table["repo"].as_str(), Some("../web"));
        assert_eq!(table["rev"].as_str(), Some("abc"));
    }
}
