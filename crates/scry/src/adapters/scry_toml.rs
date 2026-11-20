//! scry's settings, read from the project: the `[scry]` table of
//! `.sorcerer/config.toml` (parsed when the project opens), and the
//! steering in `.sorcerer/scry.toml` with the files it includes.

use std::path::{Component, Path, PathBuf};

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::config::{ScryConfig, scry_config};
use crate::domain::steering::{Include, Steering, parse_steering};
use crate::ports::settings::Settings;

/// Where the steering lives, relative to the project root.
pub const STEERING_FILE: &str = ".sorcerer/scry.toml";

/// Where the steering lived before. A file still there is an error, so
/// that it is not silently ignored.
const OLD_STEERING_FILE: &str = ".sorcerer/scry.md";

pub struct ScryToml {
    project: Project,
}

impl ScryToml {
    /// The project's settings, read when asked for.
    pub fn for_project(project: &Project) -> ScryToml {
        ScryToml {
            project: project.clone(),
        }
    }
}

impl Settings for ScryToml {
    fn scry_config(&self) -> Result<ScryConfig> {
        scry_config(&self.project)
    }

    /// The project's steering, read and checked:
    ///
    /// 1. an old `.sorcerer/scry.md` is an error, so it is not silently
    ///    ignored;
    /// 2. no `.sorcerer/scry.toml` means no steering;
    /// 3. the file is parsed and checked ([`parse_steering`]);
    /// 4. each file it includes is read, in order. An include must be a
    ///    path inside the project, not fenced off, and UTF-8 text;
    /// 5. the whole must fit under the 32 KB cap.
    fn steering(&self) -> Result<Steering> {
        self.refuse_old_steering_file()?;
        let file = self.project.root.join(STEERING_FILE);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Steering::default()),
            Err(e) => return Err(Error::io(&file, e)),
        };
        let raw = parse_steering(&text, &file)?;
        let fences = Fences::for_project(&self.project)?;
        let includes = raw
            .include
            .iter()
            .enumerate()
            .map(|(i, path)| self.read_include(&file, &fences, i + 1, path))
            .collect::<Result<Vec<Include>>>()?;
        let steering = Steering {
            context: raw.context,
            includes,
            rules: raw.rules,
        };
        match steering.over_cap() {
            Some(why) => Err(Error::Config { file, why }),
            None => Ok(steering),
        }
    }

    /// The steering file, when the project has one.
    fn steering_source(&self) -> Option<PathBuf> {
        let file = self.project.root.join(STEERING_FILE);
        file.exists().then_some(file)
    }
}

impl ScryToml {
    fn refuse_old_steering_file(&self) -> Result<()> {
        let old = self.project.root.join(OLD_STEERING_FILE);
        if !old.exists() {
            return Ok(());
        }
        Err(Error::Config {
            file: old,
            why: "no longer read: steering is .sorcerer/scry.toml (context, include, [[rule]] with id and text); see the user book's Steering section".into(),
        })
    }

    /// The `n`th include (counting from 1) of the steering `file`, read.
    /// An error names the include and why it cannot be read.
    ///
    /// Example: `include[2] ../notes.md: an include may not leave the
    /// project root`.
    fn read_include(&self, file: &Path, fences: &Fences, n: usize, path: &Path) -> Result<Include> {
        let refuse = |why: &str| Error::Config {
            file: file.to_path_buf(),
            why: format!("include[{n}] {}: {why}", path.display()),
        };
        if path.is_absolute() {
            return Err(refuse("an include is a path relative to the project root"));
        }
        if path.components().any(|c| c == Component::ParentDir) {
            return Err(refuse("an include may not leave the project root"));
        }
        if !fences.allows(path) {
            return Err(refuse(
                "fenced (outside the root, under .sorcerer/ or .git/, a secret, or [model] not_evidence)",
            ));
        }
        let bytes = match std::fs::read(self.project.root.join(path)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(refuse("not found")),
            Err(e) => return Err(refuse(&e.to_string())),
        };
        let text = String::from_utf8(bytes).map_err(|_| refuse("not UTF-8"))?;
        Ok(Include {
            path: path.to_path_buf(),
            text,
        })
    }
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &[u8])]) -> (tempfile::TempDir, ScryToml) {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".sorcerer")).unwrap();
        for (path, bytes) in files {
            let p = d.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
        let p = Project::open(d.path()).unwrap();
        (d, ScryToml::for_project(&p))
    }

    fn steering_error(files: &[(&str, &[u8])]) -> String {
        project(files).1.steering().unwrap_err().to_string()
    }

    #[test]
    fn no_file_is_empty_steering() {
        let (_d, s) = project(&[]);
        assert_eq!(s.steering().unwrap(), Steering::default());
        assert_eq!(s.steering_source(), None);
    }

    #[test]
    fn an_old_scry_md_is_an_error_pointing_to_scry_toml() {
        for files in [
            &[(".sorcerer/scry.md", b"## Rules\n".as_slice())][..],
            &[
                (".sorcerer/scry.md", b"## Rules\n".as_slice()),
                (".sorcerer/scry.toml", b"context = \"c\"\n".as_slice()),
            ][..],
        ] {
            let e = steering_error(files);
            assert!(e.contains("scry.md"), "{e}");
            assert!(e.contains("no longer read"), "{e}");
            assert!(e.contains(".sorcerer/scry.toml"), "{e}");
        }
    }

    #[test]
    fn includes_are_read_in_order_under_the_fences() {
        let (_d, s) = project(&[
            (
                ".sorcerer/scry.toml",
                b"include = [\"docs/b.md\", \"docs/a.md\"]\n[[rule]]\nid = \"r\"\ntext = \"t\"\n",
            ),
            ("docs/a.md", b"A\n"),
            ("docs/b.md", b"B\n"),
        ]);
        let steering = s.steering().unwrap();
        let got: Vec<(&Path, &str)> = steering
            .includes
            .iter()
            .map(|i| (i.path.as_path(), i.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (Path::new("docs/b.md"), "B\n"),
                (Path::new("docs/a.md"), "A\n")
            ]
        );
        assert!(
            s.steering_source()
                .unwrap()
                .ends_with(".sorcerer/scry.toml")
        );
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("x.md"), "x").unwrap();
        for (include, extra) in [
            (".env", &[(".env", b"SECRET=1".as_slice())][..]),
            (
                ".sorcerer/notes.md",
                &[(".sorcerer/notes.md", b"n".as_slice())][..],
            ),
            ("../x.md", &[][..]),
            ("/etc/hosts", &[][..]),
            ("link.md", &[][..]),
        ] {
            let toml = format!("include = [\"docs/a.md\", \"{include}\"]\n");
            let mut files: Vec<(&str, &[u8])> = vec![
                (".sorcerer/scry.toml", toml.as_bytes()),
                ("docs/a.md", b"A"),
            ];
            files.extend_from_slice(extra);
            let (d, s) = project(&files);
            if include == "link.md" {
                std::os::unix::fs::symlink(outside.path().join("x.md"), d.path().join("link.md"))
                    .unwrap();
            }
            let e = s.steering().unwrap_err().to_string();
            assert!(
                e.contains(&format!("include[2] {include}")),
                "{include}: {e}"
            );
            assert!(e.contains("scry.toml"), "{include}: {e}");
        }
    }

    #[test]
    fn a_missing_or_binary_include_is_an_error() {
        let e = steering_error(&[(".sorcerer/scry.toml", b"include = [\"docs/x.md\"]\n")]);
        assert!(e.contains("include[1] docs/x.md: not found"), "{e}");
        let e = steering_error(&[
            (".sorcerer/scry.toml", b"include = [\"logo.png\"]\n"),
            ("logo.png", &[0xff, 0xfe, 0x00]),
        ]);
        assert!(e.contains("include[1] logo.png: not UTF-8"), "{e}");
    }

    #[test]
    fn steering_over_the_cap_is_an_error() {
        let big = "x".repeat(crate::domain::steering::STEERING_CAP);
        let e = steering_error(&[
            (".sorcerer/scry.toml", b"include = [\"docs/big.md\"]\n"),
            ("docs/big.md", big.as_bytes()),
        ]);
        assert!(e.contains("32768"), "{e}");
        assert!(e.contains("include docs/big.md"), "{e}");
    }

    #[test]
    fn the_repository_steering_parses() {
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.sorcerer/scry.toml");
        let raw = parse_steering(&std::fs::read_to_string(&file).unwrap(), &file).unwrap();
        let ids: Vec<&str> = raw.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "usecases-are-the-api",
                "domain-is-pure",
                "usecases-depend-inward",
                "ports-are-traits",
                "adapters-implement-ports",
                "cli-is-thin",
                "one-name-per-concept",
                "one-level-per-body",
                "helpers-say-intent",
                "log-text-apart",
                "plain-names",
                "comments-say-why",
                "invariants-are-types",
                "immutable-by-default",
                "the-engine-knows-no-language",
                "nothing-outside-the-repository",
                "books-change-with-the-code",
            ]
        );
        assert!(raw.context.starts_with("Sorcerer is a Rust workspace"));
        assert!(raw.include.is_empty());
    }
}
