//! Whether scry can run in a project: provider, Jev, languages, steering,
//! store and ledger, a row each. Reads only; writes nothing.

use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::json_store::Snapshot;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;
use sorcerer_core::ports::ledger::Ledger;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_models::adapters::jev::Jev;
use sorcerer_models::adapters::providers::{default_decider, default_provider_for};

use crate::domain::finding::Status;
use crate::domain::log_text::count;
use crate::ports::finding_store::FindingStore;
use crate::ports::settings::Settings;

/// Whether scry can run in PROJECT, a row each: `provider`, `jev`,
/// `languages` (each language the project's files use loads), `steering`,
/// `store`, `ledger`. Reads only; writes nothing.
pub fn check_setup(
    project: &Project,
    env: &dyn Env,
    cache: &GrammarCache,
    store: &dyn FindingStore,
    ledger: &dyn Ledger,
    settings: &dyn Settings,
) -> Vec<(String, Result<String, String>)> {
    let provider = default_provider_for(project, env)
        .map(|(_, c)| format!("{} ({})", c.name, c.why))
        .map_err(|e| e.to_string());
    let jev = match Jev::from_env(&project.config.decide, env, None) {
        Ok(_) => Ok(default_decider(&project.config.decide, env)
            .map(|c| format!("{} ({})", c.name, c.why))
            .unwrap_or_else(|| "jev".into())),
        Err(e) => Err(format!("{e}; scry does not run without Jev")),
    };
    let steering_row = settings
        .steering()
        .map(|s| {
            let file = if project.root.join(".sorcerer/scry.md").exists() {
                "with .sorcerer/scry.md"
            } else {
                "no .sorcerer/scry.md (the shipped text only)"
            };
            format!("{}, {file}", count(s.rules.len(), "rule"))
        })
        .map_err(|e| e.to_string());
    let store_row = {
        let st = store;
        match st.read() {
            Ok(Snapshot::Absent) => Ok("no findings yet".into()),
            Ok(Snapshot::Current(s)) => {
                let open = s
                    .findings
                    .iter()
                    .filter(|f| f.status == Status::Open)
                    .count();
                Ok(format!(
                    "{}, {open} open",
                    count(s.findings.len(), "finding")
                ))
            }
            Ok(Snapshot::Unreadable(why)) => Err(format!(
                "{} cannot be read ({why}); scry does not run until it is fixed or moved aside",
                st.path().display()
            )),
            Err(e) => Err(e.to_string()),
        }
    };
    let ledger_row = match ledger.read() {
        Ok(rows) => {
            let scry = rows.errands.iter().filter(|e| e.product == "scry").count();
            let mut s = format!(
                "{}, {scry} of them scry's",
                count(rows.errands.len(), "row")
            );
            if rows.skipped > 0 {
                s.push_str(&format!("; {} did not read", count(rows.skipped, "line")));
            }
            Ok(s)
        }
        Err(e) => Err(e.to_string()),
    };
    vec![
        ("provider".into(), provider),
        ("jev".into(), jev),
        ("languages".into(), languages_row(project, cache)),
        ("steering".into(), steering_row),
        ("store".into(), store_row),
        ("ledger".into(), ledger_row),
    ]
}

/// The project's languages (`[languages] names`, else those covering a
/// file in the project), each loaded through CACHE.
fn languages_row(project: &Project, cache: &GrammarCache) -> Result<String, String> {
    let names: Vec<String> = if project.config.languages.is_empty() {
        let view = FilesView::new(std::sync::Arc::new(project.clone()), cache.clone());
        let files = view.glob("**").map_err(|e| e.to_string())?;
        let mut used: Vec<String> = files
            .iter()
            .filter_map(|f| project.languages.for_path(f).map(|l| l.name.clone()))
            .collect();
        used.sort();
        used.dedup();
        used
    } else {
        project.config.languages.clone()
    };
    if names.is_empty() {
        return Ok("no file here is in a language sorcerer knows".into());
    }
    let failed: Vec<String> = names
        .iter()
        .filter_map(|n| match project.languages.load(n, cache) {
            Ok(_) => None,
            Err(e) => Some(format!("{n}: {e}")),
        })
        .collect();
    if failed.is_empty() {
        Ok(format!("{} load", names.join(", ")))
    } else {
        Err(failed.join("; "))
    }
}
