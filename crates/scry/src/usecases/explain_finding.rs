//! A finding in full: the rule it cites and its text, and each cited
//! range's file when the fences allow it.

use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::finding::{Evidence, Finding};
use crate::ports::finding_store::FindingStore;
use crate::ports::settings::Settings;
use crate::usecases::read_findings;

/// A cited file's text, or why it is not shown.
#[derive(Debug, Clone, PartialEq)]
pub enum CitedText {
    /// Fenced, or outside the project: never read.
    NotShown,
    Unreadable,
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub finding: Finding,
    /// The rule's name as the steering has it, and its text when it has it.
    pub rule: Option<(String, Option<String>)>,
    pub evidence: Vec<(Evidence, CitedText)>,
}

/// The finding ID (or a unique prefix of four or more); an unknown or
/// ambiguous id is an error saying which.
pub fn explain_finding(
    project: &Project,
    store: &dyn FindingStore,
    settings: &dyn Settings,
    view: &dyn CodebaseView,
    id: &str,
) -> anyhow::Result<Explanation> {
    let st = read_findings(store)?;
    let Some(f) = st.get(id) else {
        let n = st
            .findings
            .iter()
            .filter(|f| id.len() >= 4 && f.id.0.starts_with(id))
            .count();
        if n > 1 {
            anyhow::bail!("{id} matches more than one finding");
        }
        anyhow::bail!("no finding {id}");
    };
    let rule = match &f.rule {
        None => None,
        Some(r) => {
            let st = settings.steering()?;
            let name = st
                .rule_text(r)
                .map(|_| r.as_str())
                .or_else(|| st.resolve_rule(r))
                .unwrap_or(r.as_str())
                .to_string();
            let text = st.rule_text(&name).map(|t| t.trim().to_string());
            Some((name, text))
        }
    };
    let mut evidence = Vec::new();
    if !f.evidence.is_empty() {
        // a citation the checks never saw may name any path: read only what
        // the fences allow, inside the root
        let fences = Fences::for_project(project)?;
        for e in &f.evidence {
            let text = if e.file.is_absolute() || !fences.allows(&e.file) {
                CitedText::NotShown
            } else {
                match view.text(&e.file) {
                    Ok(t) => CitedText::Text(t),
                    Err(_) => CitedText::Unreadable,
                }
            };
            evidence.push((e.clone(), text));
        }
    }
    Ok(Explanation {
        finding: f.clone(),
        rule,
        evidence,
    })
}
