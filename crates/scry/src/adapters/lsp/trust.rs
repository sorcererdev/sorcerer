//! The trust gate: a project whose own config runs code or moves the API key (a provider's
//! `runner` or `url`, a grammar of its own) is not read, parsed or reviewed by the editor's
//! server until the user trusts it.
//!
//! Every project that sets none of those needs no trust, so most projects never meet the
//! gate. For one that does, the server asks once a session, at the first save of a file the
//! project has a language for: `Trust` puts the project on the user's trust list and reviews
//! what was saved meanwhile; `Not now`, or closing the question, leaves the project alone until
//! the next session. An editor that cannot show a question with answers gets a plain message
//! saying how to trust the project from a terminal (`scry trust`).
//!
//! The text the gate says is built last in the file.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lsp_types::MessageType;
use sorcerer_core::domain::project::Project;
use sorcerer_core::domain::trust::{TrustReason, needs_trust};
use sorcerer_lsp::Answer;

use super::{Server, lock};
use crate::adapters::wiring::Wiring;

/// What the gate has done this session.
#[derive(Default)]
pub(super) struct TrustQuestion {
    /// The question was asked (or the fallback said); it is never asked twice.
    asked: bool,
    /// The files saved while the question waits for an answer, to review on *Trust*.
    waiting: BTreeSet<PathBuf>,
    /// The user said *Not now*, or closed the question: nothing more this session.
    declined: bool,
}

impl<W: Wiring + 'static> Server<W> {
    /// Whether the server may read the project's files: it needs no trust, or the user
    /// trusted it. Asks nothing.
    pub(super) fn trusted(&self, project: &Project) -> bool {
        needs_trust(project).is_empty() || self.on_trust_list(project)
    }

    /// The same, at a save: when the project is not trusted, the files saved are kept for a
    /// review after *Trust*, and the question is asked if this session has not asked it.
    pub(super) fn trusted_at_save(self: &Arc<Self>, project: &Project, saved: &[PathBuf]) -> bool {
        let reasons = needs_trust(project);
        if reasons.is_empty() || self.on_trust_list(project) {
            return true;
        }
        let ask_now = {
            let mut question = lock(&self.trust_question);
            if question.declined {
                return false;
            }
            question.waiting.extend(saved.iter().cloned());
            !std::mem::replace(&mut question.asked, true)
        };
        if ask_now {
            self.ask_for_trust(project, &reasons);
        }
        false
    }

    fn on_trust_list(&self, project: &Project) -> bool {
        self.cfg
            .trust
            .as_ref()
            .is_some_and(|list| list.is_trusted(&project.root))
    }

    /// The question, or the fallback message when there is no trust list to write.
    fn ask_for_trust(self: &Arc<Self>, project: &Project, reasons: &[TrustReason]) {
        if self.cfg.trust.is_none() {
            self.say_how_to_trust(reasons);
            return;
        }
        let server = Arc::clone(self);
        let root = project.root.clone();
        let fallback_keys = reasons.to_vec();
        self.client
            .ask(&question_text(reasons), &[TRUST, NOT_NOW], move |answer| {
                server.answered(&root, answer, &fallback_keys);
            });
    }

    /// The user's answer: *Trust* trusts and reviews what waited; anything else waits for
    /// the next session; an editor that cannot ask gets the fallback.
    fn answered(self: &Arc<Self>, root: &Path, answer: Answer, reasons: &[TrustReason]) {
        match answer {
            Answer::Picked(picked) if picked == TRUST => self.trust_and_review(root),
            Answer::Picked(_) | Answer::Closed => {
                let mut question = lock(&self.trust_question);
                question.declined = true;
                question.waiting.clear();
            }
            Answer::Unsupported => {
                // nothing will ever answer for the files that waited
                lock(&self.trust_question).waiting.clear();
                self.say_how_to_trust(reasons);
            }
        }
    }

    fn trust_and_review(self: &Arc<Self>, root: &Path) {
        let Some(list) = &self.cfg.trust else {
            return;
        };
        if let Err(e) = list.trust(root) {
            // nothing will review what waited: say how to trust from a terminal instead
            lock(&self.trust_question).waiting.clear();
            let file = list.file().display().to_string();
            self.client.say_once(
                "trust",
                MessageType::WARNING,
                &not_written_text(&file, &e.to_string()),
            );
            return;
        }
        let waiting = std::mem::take(&mut lock(&self.trust_question).waiting);
        self.publish_open();
        for file in waiting {
            self.queue(file);
        }
    }

    fn say_how_to_trust(&self, reasons: &[TrustReason]) {
        self.client
            .say_once("trust", MessageType::WARNING, &fallback_text(reasons));
    }
}

// The text the gate says.

const TRUST: &str = "Trust";
const NOT_NOW: &str = "Not now";

/// How long a value may be in the question before it is cut.
const LONGEST_VALUE: usize = 60;

/// The question: each setting on its own line, a long value cut.
///
/// Example: `scry: this project's config sets\n[model.providers.mine] runner = /x/run\nTrust
/// this project?`.
fn question_text(reasons: &[TrustReason]) -> String {
    let settings: String = reasons
        .iter()
        .map(|r| format!("{} = {}\n", r.key, cut(&r.value)))
        .collect();
    format!("scry: this project's config sets\n{settings}Trust this project?")
}

/// A value of at most 60 characters, else its first 60 and `…`.
fn cut(value: &str) -> String {
    if value.chars().count() <= LONGEST_VALUE {
        value.to_string()
    } else {
        let kept: String = value.chars().take(LONGEST_VALUE).collect();
        format!("{kept}…")
    }
}

fn not_written_text(file: &str, why: &str) -> String {
    format!(
        "scry is not reviewing: the trust list {file} could not be written ({why}). Run \"scry trust\" in this project to allow that."
    )
}

fn fallback_text(reasons: &[TrustReason]) -> String {
    let keys: Vec<&str> = reasons.iter().map(|r| r.key.as_str()).collect();
    format!(
        "scry is not reviewing: this project's config sets {}. Run \"scry trust\" in this project to allow that.",
        keys.join(", ")
    )
}
