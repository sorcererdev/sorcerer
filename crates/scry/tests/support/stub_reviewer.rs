//! A reviewer that answers what the test tells it to.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Mutex;

use scry::domain::review_prompt::ReviewAsk;
use scry::ports::reviewer::{Reviewed, Reviewer};
use sorcerer_core::Cancel;

use super::errand;

type Hook = Box<dyn Fn(&ReviewAsk, &Cancel) + Send + Sync>;

/// The review call's stand-in: replies by the set of files in the batch (an
/// empty findings list for a batch it has no reply for), and records every ask.
/// A hook, when set, runs during each call (after the ask is recorded).
pub struct StubReviewer {
    replies: Vec<(BTreeSet<PathBuf>, String)>,
    pub cost: f64,
    pub asks: Mutex<Vec<ReviewAsk>>,
    /// The cap each call was given.
    pub caps: Mutex<Vec<f64>>,
    timeout: bool,
    hook: Option<Hook>,
    /// A call whose cancel is set by the time its hook returns fails as cancelled, as a live
    /// reviewer's would.
    honours_cancel: bool,
}

impl StubReviewer {
    pub fn new() -> StubReviewer {
        StubReviewer {
            replies: Vec::new(),
            cost: 0.04,
            asks: Mutex::new(Vec::new()),
            caps: Mutex::new(Vec::new()),
            timeout: false,
            hook: None,
            honours_cancel: false,
        }
    }

    /// Every call times out, as a hung `claude -p` would.
    pub fn timing_out(mut self) -> StubReviewer {
        self.timeout = true;
        self
    }

    /// Runs HOOK during every review call, as a user acting mid-pass would.
    pub fn on_review(mut self, hook: impl Fn(&ReviewAsk) + Send + Sync + 'static) -> StubReviewer {
        self.hook = Some(Box::new(move |ask, _| hook(ask)));
        self
    }

    /// Runs HOOK during every review call with the call's cancel; a call cancelled by the
    /// time HOOK returns fails as cancelled.
    pub fn on_review_cancellable(
        mut self,
        hook: impl Fn(&ReviewAsk, &Cancel) + Send + Sync + 'static,
    ) -> StubReviewer {
        self.hook = Some(Box::new(hook));
        self.honours_cancel = true;
        self
    }

    /// Replies REPLY to a batch whose files are exactly FILES.
    pub fn reply(mut self, files: &[&str], reply: &str) -> StubReviewer {
        let set = files.iter().map(PathBuf::from).collect();
        self.replies.push((set, reply.to_string()));
        self
    }

    pub fn asks(&self) -> Vec<ReviewAsk> {
        self.asks.lock().unwrap().clone()
    }
}

impl Default for StubReviewer {
    fn default() -> StubReviewer {
        StubReviewer::new()
    }
}

impl Reviewer for StubReviewer {
    fn review(
        &self,
        ask: &ReviewAsk,
        model: &str,
        cap_usd: f64,
        cancel: &Cancel,
    ) -> sorcerer_models::Result<Reviewed> {
        self.asks.lock().unwrap().push(ask.clone());
        self.caps.lock().unwrap().push(cap_usd);
        if self.timeout {
            return Err(sorcerer_models::Error::Timeout {
                provider: "stub".into(),
                seconds: 420,
            });
        }
        if let Some(h) = &self.hook {
            h(ask, cancel);
        }
        if self.honours_cancel && cancel.is_cancelled() {
            return Err(sorcerer_models::Error::Cancelled);
        }
        let files: BTreeSet<PathBuf> = ask.units.iter().map(|u| u.file.clone()).collect();
        let reply = self
            .replies
            .iter()
            .find(|(f, _)| *f == files)
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| r#"{"findings": [], "open": []}"#.to_string());
        Ok(Reviewed {
            reply,
            errand: errand("review", model, self.cost),
        })
    }
}
