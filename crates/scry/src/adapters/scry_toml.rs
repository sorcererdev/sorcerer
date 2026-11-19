//! Scry's settings from the project: `[scry]` in `.sorcerer/config.toml`
//! (already parsed with the project) and the steering in
//! `.sorcerer/scry.toml`, its includes read under the fences.

use std::path::{Component, Path, PathBuf};

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::{Error, Result};

use crate::domain::config::{ScryConfig, scry_config};
use crate::domain::steering::{Include, Steering, parse_steering};
use crate::ports::settings::Settings;

/// Where the steering lives, relative to the project root.
pub const STEERING_FILE: &str = ".sorcerer/scry.toml";

/// Where it lived before; still there is an error, so it is not silently
/// ignored.
const OLD_STEERING_FILE: &str = ".sorcerer/scry.md";

pub struct ScryToml {
    project: Project,
}

impl ScryToml {
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

    /// `.sorcerer/scry.toml` checked, each include read in order (relative,
    /// inside the root, not fenced, text), and the whole under the cap.
    /// No file is no steering; an old `.sorcerer/scry.md` is an error.
    fn steering(&self) -> Result<Steering> {
        let root = &self.project.root;
        let old = root.join(OLD_STEERING_FILE);
        if old.exists() {
            return Err(Error::Config {
                file: old,
                why: "no longer read: steering is .sorcerer/scry.toml (context, include, [[rule]] with id and text); see the user book's Steering section".into(),
            });
        }
        let file = root.join(STEERING_FILE);
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Steering::default()),
            Err(e) => return Err(Error::io(&file, e)),
        };
        let raw = parse_steering(&text, &file)?;
        let bad = |n: usize, p: &Path, why: &str| Error::Config {
            file: file.clone(),
            why: format!("include[{n}] {}: {why}", p.display()),
        };
        let fences = Fences::for_project(&self.project)?;
        let mut includes = Vec::new();
        for (i, p) in raw.include.iter().enumerate() {
            let n = i + 1;
            if p.is_absolute() {
                return Err(bad(
                    n,
                    p,
                    "an include is a path relative to the project root",
                ));
            }
            if p.components().any(|c| c == Component::ParentDir) {
                return Err(bad(n, p, "an include may not leave the project root"));
            }
            if !fences.allows(p) {
                return Err(bad(
                    n,
                    p,
                    "fenced (outside the root, under .sorcerer/ or .git/, a secret, or [model] not_evidence)",
                ));
            }
            let full = root.join(p);
            let bytes = match std::fs::read(&full) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(bad(n, p, "not found"));
                }
                Err(e) => return Err(bad(n, p, &e.to_string())),
            };
            let text = String::from_utf8(bytes).map_err(|_| bad(n, p, "not UTF-8"))?;
            includes.push(Include {
                path: p.clone(),
                text,
            });
        }
        let st = Steering {
            context: raw.context,
            includes,
            rules: raw.rules,
        };
        if let Some(why) = st.over_cap() {
            return Err(Error::Config { file, why });
        }
        Ok(st)
    }

    fn steering_source(&self) -> Option<PathBuf> {
        let file = self.project.root.join(STEERING_FILE);
        file.exists().then_some(file)
    }
}

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
        let st = s.steering().unwrap();
        let got: Vec<(&Path, &str)> = st
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
                "the-engine-knows-no-language",
                "nothing-outside-the-repository",
                "books-change-with-the-code",
            ]
        );
        assert!(raw.context.starts_with("Sorcerer is a Rust workspace"));
        assert!(raw.include.is_empty());
    }
}
