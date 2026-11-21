//! What in a project's own files must be trusted before a server that starts by itself (an
//! editor's) may act on it.
//!
//! Three things a project can set run code or move the user's key: a provider's `runner` (a
//! program sorcerer starts), a provider's `url` (where the code and the API key are sent) and
//! a grammar the project's own `language.toml` names (C source sorcerer compiles and loads).
//! A project that sets none of them needs no trust. The command line never asks: typing a
//! command in a project is the consent.

use crate::domain::language::GrammarSource;
use crate::domain::project::Project;

/// One setting that makes a project need trust: where it is set and what it says.
///
/// Example: `key` is `[model.providers.mine] runner` and `value` is `/x/run`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustReason {
    pub key: String,
    pub value: String,
}

/// Every setting in the project's own files that needs trust, in a fixed order: each
/// provider's `runner` then its `url` (providers by name), then each language whose grammar
/// the project set (languages by name). Empty when the project needs no trust.
///
/// Example: a config with `[model.providers.mine] runner = "/x/run"` and a
/// `.sorcerer/languages/toy/language.toml` with `grammar = { path = "g" }` gives two reasons,
/// `[model.providers.mine] runner = /x/run` and
/// `.sorcerer/languages/toy/language.toml grammar path = <root>/.sorcerer/languages/toy/g` (a
/// grammar path is shown as the language file resolved it). A shipped language, or a project
/// file that changes only a shipped language's `files`, gives none.
pub fn needs_trust(project: &Project) -> Vec<TrustReason> {
    provider_reasons(project)
        .chain(grammar_reasons(project))
        .collect()
}

/// A `runner` or `url` in each `[model.providers.<name>]` table.
fn provider_reasons(project: &Project) -> impl Iterator<Item = TrustReason> + '_ {
    project
        .config
        .model
        .providers
        .iter()
        .flat_map(|(name, provider)| {
            let runner = provider.runner.as_ref().map(|r| TrustReason {
                key: format!("[model.providers.{name}] runner"),
                value: r.display().to_string(),
            });
            let url = provider.url.as_ref().map(|u| TrustReason {
                key: format!("[model.providers.{name}] url"),
                value: u.clone(),
            });
            runner.into_iter().chain(url)
        })
}

/// A grammar set by the project's own language file, fetched (`url`, with or without a pinned
/// revision) or built from a directory (`path`).
fn grammar_reasons(project: &Project) -> impl Iterator<Item = TrustReason> + '_ {
    project
        .languages
        .names()
        .into_iter()
        .filter_map(|name| project.languages.get(name))
        .filter(|language| language.origins.get("grammar").map(String::as_str) == Some("project"))
        .map(|language| {
            let file = format!(".sorcerer/languages/{}/language.toml", language.name);
            match &language.grammar {
                GrammarSource::Git { url, .. } => TrustReason {
                    key: format!("{file} grammar url"),
                    value: url.clone(),
                },
                GrammarSource::Local(path) => TrustReason {
                    key: format!("{file} grammar path"),
                    value: path.display().to_string(),
                },
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Project) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".sorcerer")).unwrap();
        for (rel, text) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let project = Project::open(dir.path()).unwrap();
        (dir, project)
    }

    #[test]
    fn a_project_with_no_config_needs_no_trust() {
        let (_dir, project) = project_with(&[]);
        assert!(needs_trust(&project).is_empty());
    }

    #[test]
    fn a_runner_and_a_url_are_each_a_reason() {
        let (_dir, project) = project_with(&[(
            ".sorcerer/config.toml",
            "[model.providers.mine]\nrunner = \"/x/run\"\nurl = \"https://e.example\"\n",
        )]);
        let reasons = needs_trust(&project);
        assert_eq!(
            reasons,
            vec![
                TrustReason {
                    key: "[model.providers.mine] runner".into(),
                    value: "/x/run".into()
                },
                TrustReason {
                    key: "[model.providers.mine] url".into(),
                    value: "https://e.example".into()
                },
            ]
        );
    }

    #[test]
    fn a_shipped_language_needs_no_trust_and_a_project_grammar_does() {
        let (_dir, project) = project_with(&[(
            ".sorcerer/languages/toy/language.toml",
            "files = [\"**/*.toy\"]\ngrammar = { path = \"g\" }\n",
        )]);
        let reasons = needs_trust(&project);
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        assert!(reasons[0].key.ends_with("grammar path"), "{reasons:?}");
        assert_eq!(
            reasons[0].key,
            ".sorcerer/languages/toy/language.toml grammar path"
        );
        // an absolute path in the platform's own form: compared by its components
        assert!(
            std::path::Path::new(&reasons[0].value).ends_with("languages/toy/g"),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_fetched_grammar_needs_trust_with_its_revision_pinned() {
        let (_dir, project) = project_with(&[(
            ".sorcerer/languages/toy/language.toml",
            "files = [\"**/*.toy\"]\ngrammar = { git = \"https://g.example/toy\", rev = \"abc\" }\n",
        )]);
        let reasons = needs_trust(&project);
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        assert_eq!(
            reasons[0].key,
            ".sorcerer/languages/toy/language.toml grammar url"
        );
        assert_eq!(reasons[0].value, "https://g.example/toy");
    }
}
