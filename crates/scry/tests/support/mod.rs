//! Helpers and doubles shared by scry's integration tests: git fixtures (as
//! core's), a project on the vendored grammars, and stand-ins for the review
//! call, the decider and the clock.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, FixedOffset, Utc};
use scry::adapters::json_finding_store::JsonFindingStore;
use scry::adapters::scry_toml::ScryToml;
use scry::domain::findings::Findings;
use scry::domain::review_prompt::ReviewAsk;
use scry::ports::clock::Clock;
use scry::ports::finding_store::FindingStore;
use scry::ports::reviewer::{Reviewed, Reviewer};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::json_store::Snapshot;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::domain::project::Project;
use sorcerer_models::domain::decide::{Answer, Confidence, Question, Weighed};
use sorcerer_models::ports::decider::Decider;
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

type Hook = Box<dyn Fn(&ReviewAsk) + Send + Sync>;

/// The review call's stand-in: replies by the set of files in the pack (an
/// empty findings list for a pack it has no reply for), and records every ask.
/// A hook, when set, runs during each call (after the ask is recorded).
pub struct StubReviewer {
    replies: Vec<(BTreeSet<PathBuf>, String)>,
    pub cost: f64,
    pub asks: Mutex<Vec<ReviewAsk>>,
    /// The cap each call was given.
    pub caps: Mutex<Vec<f64>>,
    timeout: bool,
    hook: Option<Hook>,
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
        }
    }

    /// Every call times out, as a hung `claude -p` would.
    pub fn timing_out(mut self) -> StubReviewer {
        self.timeout = true;
        self
    }

    /// Runs HOOK during every review call, as a user acting mid-pass would.
    pub fn on_review(mut self, hook: impl Fn(&ReviewAsk) + Send + Sync + 'static) -> StubReviewer {
        self.hook = Some(Box::new(hook));
        self
    }

    /// Replies REPLY to a pack whose files are exactly FILES.
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
        _cancel: &Cancel,
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
            h(ask);
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

/// The ports a review talks through, over PROJECT's files.
pub struct Ports {
    pub store: JsonFindingStore,
    pub ledger: JsonlLedger,
    pub settings: ScryToml,
}

impl Ports {
    pub fn of(project: &Project) -> Ports {
        Ports {
            store: JsonFindingStore::for_project(project).unwrap(),
            ledger: JsonlLedger::for_project(project).unwrap(),
            settings: ScryToml::for_project(project),
        }
    }
}

/// PROJECT's finding store.
pub fn store_of(project: &Project) -> JsonFindingStore {
    JsonFindingStore::for_project(project).unwrap()
}

/// F applied to PROJECT's store under its lock; what F gave.
pub fn update_store<R>(
    project: &Project,
    f: impl FnOnce(&mut Findings) -> R,
) -> sorcerer_core::Result<R> {
    let mut f = Some(f);
    let mut out = None;
    store_of(project).update(&mut |s| out = f.take().map(|f| f(s)))?;
    Ok(out.expect("the update ran"))
}

/// PROJECT's findings as stored (none when there is no store yet).
pub fn read_store(project: &Project) -> Findings {
    match store_of(project).read().unwrap() {
        Snapshot::Current(s) => s,
        Snapshot::Absent => Findings::default(),
        Snapshot::Unreadable(why) => panic!("{why}"),
    }
}
