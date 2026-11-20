//! One finding in full, for `scry why`: the finding, the rule it cites with
//! its text, and the files its evidence cites, where the fences allow.

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
    /// Allowed, but the file could not be read.
    Unreadable,
    Text(String),
}

/// A finding with what explains it.
#[derive(Debug, Clone, PartialEq)]
pub struct Explanation {
    pub finding: Finding,
    /// The rule's id, and its text when the steering still has it.
    pub rule: Option<(String, Option<String>)>,
    pub evidence: Vec<(Evidence, CitedText)>,
}

/// One finding in full, found by its id or the start of it (at least four
/// characters, matching only one finding):
///
/// 1. the finding, or an error saying no finding or more than one matches;
/// 2. the rule it cites, with the rule's text if the steering still has it;
/// 3. the text of each file its evidence cites, where the fences allow it.
pub fn explain_finding(
    project: &Project,
    store: &dyn FindingStore,
    settings: &dyn Settings,
    view: &dyn CodebaseView,
    id: &str,
) -> anyhow::Result<Explanation> {
    let stored = read_findings(store)?;
    let finding = stored.find(id).map_err(anyhow::Error::msg)?;
    Ok(Explanation {
        finding: finding.clone(),
        rule: rule_cited(finding, settings)?,
        evidence: cited_texts(finding, project, view)?,
    })
}

fn rule_cited(
    finding: &Finding,
    settings: &dyn Settings,
) -> anyhow::Result<Option<(String, Option<String>)>> {
    let Some(id) = &finding.rule else {
        return Ok(None);
    };
    let steering = settings.steering()?;
    let text = steering.rule(id).map(|rule| rule.text.trim().to_string());
    Ok(Some((id.clone(), text)))
}

/// Each piece of evidence with the text of the file it cites. A citation
/// the checks never saw may name any path, so only a relative path the
/// fences allow is read.
fn cited_texts(
    finding: &Finding,
    project: &Project,
    view: &dyn CodebaseView,
) -> anyhow::Result<Vec<(Evidence, CitedText)>> {
    if finding.evidence.is_empty() {
        return Ok(Vec::new());
    }
    let fences = Fences::for_project(project)?;
    let cited = finding.evidence.iter().map(|evidence| {
        let text = if evidence.file.is_absolute() || !fences.allows(&evidence.file) {
            CitedText::NotShown
        } else {
            match view.text(&evidence.file) {
                Ok(text) => CitedText::Text(text),
                Err(_) => CitedText::Unreadable,
            }
        };
        (evidence.clone(), text)
    });
    Ok(cited.collect())
}
