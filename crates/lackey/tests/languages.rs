//! Every shipped language's cut regions, in `corpus/languages/<name>/`: each
//! `cases/regions/<id>.toml` (`file`, and `patch`, the person's edit applied to the base with
//! `git apply`) is cut the way lackey cuts a saved change, through the real grammar, and the
//! regions equal `expected/regions/<id>.txt`. Every language has at least one case. No model, no
//! network. `LANGUAGES_BLESS=1` writes the goldens.

mod support;

use std::path::Path;

use lackey::adapters::grammar_syntax::GrammarSyntax;
use lackey::domain::cutting::{persons_changed_lines, regions_of};
use lackey::domain::region::Region;
use lackey::ports::syntax::Syntax;
use serde::Deserialize;
use support::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionsCase {
    /// The file the person edits, relative to the base.
    file: String,
    /// The edit, a patch next to the case.
    patch: String,
}

/// The regions as the golden reads them: `comment 7-7 in close_table`, then each line of the
/// region's text behind `  | `.
fn regions_text(regions: &[Region]) -> String {
    regions
        .iter()
        .map(|region| {
            let place = format!("{:?}", region.place).to_lowercase();
            let unit = region.unit.as_deref().unwrap_or("-");
            let lines: String = region
                .text
                .lines()
                .map(|line| format!("  | {line}\n"))
                .collect();
            format!(
                "{place} {}-{} in {unit}\n{lines}",
                region.lines.start, region.lines.end
            )
        })
        .collect()
}

/// The regions one case cuts.
fn cut(name: &str, dir: &Path, id: &str) -> Result<String, String> {
    let cases = dir.join("cases/regions");
    let text =
        std::fs::read_to_string(cases.join(format!("{id}.toml"))).map_err(|e| e.to_string())?;
    let case: RegionsCase = toml::from_str(&text).map_err(|e| format!("regions/{id}: {e}"))?;
    let project = TempProject::with(&[]);
    copy_folder(&dir.join("base"), &project.root);
    let before = project.read(&case.file);
    apply_patch(&project.root, &cases.join(&case.patch));
    let after = project.read(&case.file);
    let syntax = GrammarSyntax {
        languages: open_with_vendored_grammars(&project.root)
            .map_err(|e| e.to_string())?
            .languages,
        cache: test_cache(),
    };
    let file = Path::new(&case.file);
    let covered = syntax.languages.for_path(file).map(|l| l.name.clone());
    if covered.as_deref() != Some(name) {
        return Err(format!("regions/{id}: {} is not a {name} file", case.file));
    }
    let shape = syntax.shape(file, &after);
    if !shape.has_language {
        return Err(format!(
            "regions/{id}: the {name} grammar did not read {}",
            case.file
        ));
    }
    let changed = persons_changed_lines(&before, &after, &[]);
    Ok(regions_text(&regions_of(
        &case.file, &after, &changed, &shape,
    )))
}

fn case_ids(dir: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(dir.join("cases/regions"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .filter_map(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
        .collect();
    ids.sort();
    ids
}

#[test]
fn every_languages_edits_are_cut_into_the_regions_a_person_would_ask_about() {
    let mut problems = Vec::new();
    for name in shipped_language_names() {
        let dir = language_corpus(&name);
        let ids = case_ids(&dir);
        if ids.is_empty() {
            problems.push(format!("{name}: no cases/regions/*.toml"));
        }
        for id in ids {
            let golden = dir.join(format!("expected/regions/{id}.txt"));
            let outcome = cut(&name, &dir, &id).and_then(|got| matches_golden(&golden, &got));
            if let Err(problem) = outcome {
                problems.push(format!("{name}: {problem}"));
            }
        }
    }
    assert_no_problems("lackey regions", &problems);
}
