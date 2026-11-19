//! Helpers and doubles shared by scry's integration tests: git fixtures (as
//! core's), a project on the vendored grammars, and stand-ins for the find
//! call, the decider and the clock.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, FixedOffset, Utc};
use scry::find::{FindAsk, Finder, Found};
use scry::pass::Clock;
use sorcerer_core::grammar::GrammarCache;
use sorcerer_core::language::GrammarSource;
use sorcerer_core::ledger::Errand;
use sorcerer_core::view::FilesView;
use sorcerer_core::{Cancel, Project};
use sorcerer_models::decide::{Answer, Confidence, Decider, Question, Weighed};
use tempfile::TempDir;

/// Runs git in DIR, panicking with its stderr on failure.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A fresh git repository with no commit yet.
pub fn git_repo() -> TempDir {
    let d = TempDir::new().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["config", "user.name", "t"]);
    git(d.path(), &["config", "user.email", "t@example.com"]);
    d
}

/// A fresh git repository with one empty commit.
pub fn git_project() -> TempDir {
    let d = git_repo();
    git(d.path(), &["commit", "-q", "--allow-empty", "-m", "root"]);
    d
}

/// Writes TEXT at REL under DIR, creating directories.
pub fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// Commits everything in DIR.
pub fn commit(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// Core's vendored grammar sources for NAME (no network).
pub fn vendored(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sorcerer-core/tests/grammars")
        .join(name)
}

/// A grammar cache shared by every test in this process (built once).
pub fn test_cache() -> GrammarCache {
    GrammarCache::at(Path::new(env!("CARGO_TARGET_TMPDIR")).join("grammar-cache"))
}

/// The project at DIR with its grammars pointed at the vendored sources, and
/// a view of it.
pub fn project_with_vendored_grammars(dir: &Path) -> (Arc<Project>, FilesView) {
    let mut p = Project::open(dir).unwrap();
    for name in ["python", "rust"] {
        p.languages
            .override_grammar(name, GrammarSource::Local(vendored(name)));
    }
    let p = Arc::new(p);
    let v = FilesView::new(p.clone(), test_cache());
    (p, v)
}

/// An errand as a paid call would append it.
pub fn errand(verb: &str, model: &str, cost: f64) -> Errand {
    Errand {
        at: Utc::now(),
        product: "scry".into(),
        verb: verb.into(),
        model: model.into(),
        provider: "stub".into(),
        cost_usd: Some(cost),
        input_tokens: 0,
        output_tokens: 0,
        seconds: 0.0,
        ok: true,
        note: None,
    }
}

type Hook = Box<dyn Fn(&FindAsk) + Send + Sync>;

/// The find call's stand-in: replies by the set of files in the pack (an
/// empty findings list for a pack it has no reply for), and records every ask.
/// A hook, when set, runs during each call (after the ask is recorded).
pub struct StubFinder {
    replies: Vec<(BTreeSet<PathBuf>, String)>,
    pub cost: f64,
    pub asks: Mutex<Vec<FindAsk>>,
    /// The cap each call was given.
    pub caps: Mutex<Vec<f64>>,
    timeout: bool,
    hook: Option<Hook>,
}

impl StubFinder {
    pub fn new() -> StubFinder {
        StubFinder {
            replies: Vec::new(),
            cost: 0.04,
            asks: Mutex::new(Vec::new()),
            caps: Mutex::new(Vec::new()),
            timeout: false,
            hook: None,
        }
    }

    /// Every call times out, as a hung `claude -p` would.
    pub fn timing_out(mut self) -> StubFinder {
        self.timeout = true;
        self
    }

    /// Runs HOOK during every find call, as a user acting mid-pass would.
    pub fn on_find(mut self, hook: impl Fn(&FindAsk) + Send + Sync + 'static) -> StubFinder {
        self.hook = Some(Box::new(hook));
        self
    }

    /// Replies REPLY to a pack whose files are exactly FILES.
    pub fn reply(mut self, files: &[&str], reply: &str) -> StubFinder {
        let set = files.iter().map(PathBuf::from).collect();
        self.replies.push((set, reply.to_string()));
        self
    }

    pub fn asks(&self) -> Vec<FindAsk> {
        self.asks.lock().unwrap().clone()
    }
}

impl Default for StubFinder {
    fn default() -> StubFinder {
        StubFinder::new()
    }
}

impl Finder for StubFinder {
    fn find(
        &self,
        ask: &FindAsk,
        model: &str,
        cap_usd: f64,
        _cancel: &Cancel,
    ) -> sorcerer_models::Result<Found> {
        self.asks.lock().unwrap().push(ask.clone());
        self.caps.lock().unwrap().push(cap_usd);
        if self.timeout {
            return Err(sorcerer_models::Error::Timeout {
                provider: "stub".into(),
                seconds: 420,
            });
        }
        if let Some(h) = &self.hook {
            h(ask);
        }
        let files: BTreeSet<PathBuf> = ask.units.iter().map(|u| u.file.clone()).collect();
        let reply = self
            .replies
            .iter()
            .find(|(f, _)| *f == files)
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| r#"{"findings": [], "open": []}"#.to_string());
        Ok(Found {
            reply,
            errand: errand("find", model, self.cost),
        })
    }
}

type Predicate = Box<dyn Fn(&Question) -> bool + Send + Sync>;

/// The decider's stand-in: the first rule whose kind prefixes the question's
/// kind and whose predicate holds gives the answer; else a certain no.
/// Records every question asked.
pub struct StubDecider {
    rules: Vec<(String, Predicate, Answer)>,
    pub asked: Mutex<Vec<Question>>,
}

impl StubDecider {
    pub fn new() -> StubDecider {
        StubDecider {
            rules: Vec::new(),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// Answers ANSWER to questions of KIND (a prefix) for which WHEN holds.
    pub fn on(
        mut self,
        kind: &str,
        when: impl Fn(&Question) -> bool + Send + Sync + 'static,
        answer: Answer,
    ) -> StubDecider {
        self.rules.push((kind.to_string(), Box::new(when), answer));
        self
    }

    /// A yes/no answer with P(yes) = P.
    pub fn p_yes(p: f64) -> Answer {
        Answer::Yes(Confidence::Known(p))
    }

    /// The context entry NAME of Q, or "".
    pub fn context<'q>(q: &'q Question, name: &str) -> &'q str {
        q.context
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .unwrap_or("")
    }

    pub fn asked(&self) -> Vec<Question> {
        self.asked.lock().unwrap().clone()
    }
}

impl Default for StubDecider {
    fn default() -> StubDecider {
        StubDecider::new()
    }
}

impl Decider for StubDecider {
    fn ask(&self, qs: &[Question], _cancel: &Cancel) -> sorcerer_models::Result<Vec<Weighed>> {
        self.asked.lock().unwrap().extend(qs.iter().cloned());
        Ok(qs
            .iter()
            .map(|q| {
                let answer = self
                    .rules
                    .iter()
                    .find(|(k, when, _)| q.kind.starts_with(k.as_str()) && when(q))
                    .map(|(_, _, a)| a.clone())
                    .unwrap_or(Answer::No(Confidence::Known(1.0)));
                Weighed {
                    answer,
                    distribution: Vec::new(),
                }
            })
            .collect())
    }
}

/// A clock that says what it is told, in a zone it is told (UTC unless set).
pub struct FixedClock {
    at: Mutex<DateTime<Utc>>,
    zone: FixedOffset,
}

impl FixedClock {
    pub fn at(t: DateTime<Utc>) -> FixedClock {
        FixedClock {
            at: Mutex::new(t),
            zone: FixedOffset::east_opt(0).unwrap(),
        }
    }

    /// The same clock in ZONE (the user's local offset).
    pub fn in_zone(mut self, zone: FixedOffset) -> FixedClock {
        self.zone = zone;
        self
    }

    pub fn now(&self) -> DateTime<Utc> {
        *self.at.lock().unwrap()
    }

    /// Moves the clock by D (negative goes back).
    pub fn offset(&self, d: Duration) {
        *self.at.lock().unwrap() += d;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        FixedClock::now(self)
    }

    fn offset(&self) -> FixedOffset {
        self.zone
    }
}
