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
use scry::adapters::wiring::Wiring;
use scry::domain::findings::Findings;
use scry::domain::review_prompt::ReviewAsk;
use scry::ports::claim_checker::ClaimChecker;
use scry::ports::clock::Clock;
use scry::ports::finding_store::FindingStore;
use scry::ports::reviewer::{Reviewed, Reviewer};
use scry::ports::run_log::Quiet;
use scry::usecases::review_change::{Progress, ReviewDeps};
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::adapters::grammar::GrammarCache;
use sorcerer_core::adapters::json_store::Snapshot;
use sorcerer_core::adapters::jsonl_ledger::JsonlLedger;
use sorcerer_core::domain::errand::Errand;
use sorcerer_core::domain::language::GrammarSource;
use sorcerer_core::domain::project::Project;
use sorcerer_models::domain::claim::{Checked, Claim, Escalation, Verdict};
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

/// An open finding as a pass would store it: ID, of ISSUE_TYPE, about FILE on LINE, in the
/// unit UNIT (a file-level finding when `None`) that started on UNIT_START and had the
/// fingerprint PRINT, citing its own line as evidence, with confidence 0.9.
pub fn open_finding(
    id: &str,
    issue_type: &str,
    file: &str,
    unit: Option<(&str, u32, &str)>,
    line: u32,
) -> scry::domain::finding::Finding {
    use scry::domain::finding::{Evidence, Finding, FindingId, Place, Scores, Status};
    use sorcerer_core::domain::fingerprint::Fingerprint;
    use sorcerer_core::domain::unit::Span;
    let t: DateTime<Utc> = "2026-11-20T12:00:00Z".parse().unwrap();
    Finding {
        id: FindingId(id.into()),
        issue_type: issue_type.into(),
        rule: None,
        statement: format!("statement of {id}"),
        place: Place {
            file: file.into(),
            unit: unit.map(|(name, _, _)| name.to_string()),
            lines: Span {
                start: line,
                end: line,
            },
            unit_start: unit.map(|(_, start, _)| start),
            fingerprint: unit.and_then(|(_, _, print)| Fingerprint::parse(print)),
        },
        evidence: vec![Evidence {
            file: file.into(),
            lines: Span {
                start: line,
                end: line,
            },
            print: None,
        }],
        scores: Scores {
            confidence: Some(0.9),
            asked_on: None,
        },
        verdict: None,
        status: Status::Open,
        seen: 1,
        first: t,
        last: t,
        history: vec![(t, Status::Open)],
        model: "sonnet".into(),
        cost_usd: 0.0,
    }
}

/// PROJECT's store holding exactly FINDINGS.
pub fn put_findings(project: &Project, findings: Vec<scry::domain::finding::Finding>) {
    update_store(project, |s| {
        *s = Findings {
            findings,
            ..Default::default()
        };
    })
    .unwrap();
}

/// The pass's doubles: the vendored grammars' view, a stub reviewer and
/// decider, a fixed clock (2026-11-18 23:00 UTC), a claim checker that says
/// right, and one cancel for everything.
pub struct Doubles {
    pub view: FilesView,
    pub reviewer: StubReviewer,
    pub decider: StubDecider,
    pub clock: FixedClock,
    pub checker: Box<dyn ClaimChecker + Send>,
    pub ports: Ports,
    pub cancel: Cancel,
}

fn quiet(_: &Progress) {}

impl Doubles {
    pub fn new(dir: &Path, reviewer: StubReviewer) -> Doubles {
        let (project, view) = project_with_vendored_grammars(dir);
        Doubles {
            view,
            ports: Ports::of(&project),
            cancel: Cancel::default(),
            reviewer,
            decider: StubDecider::new(),
            clock: FixedClock::at("2026-11-18T23:00:00Z".parse().unwrap()),
            checker: Box::new(|_: &Claim| {
                Ok(Checked {
                    verdict: Verdict::Right,
                    why: "it does".into(),
                    citations: Vec::new(),
                    cited: Vec::new(),
                    escalation: Escalation::No,
                    errands: Vec::new(),
                })
            }),
        }
    }
}

impl Wiring for Doubles {
    fn deps(&self) -> ReviewDeps<'_> {
        ReviewDeps {
            view: &self.view,
            reviewer: &self.reviewer,
            decider: &self.decider,
            checker: &*self.checker,
            store: &self.ports.store,
            ledger: &self.ports.ledger,
            settings: &self.ports.settings,
            clock: &self.clock,
            log: &Quiet,
            progress: &quiet,
            cancel: &self.cancel,
            review_cancel: &self.cancel,
        }
    }
}

/// The command line's tests hand `main_with` a borrowed `Doubles`.
impl Wiring for &Doubles {
    fn deps(&self) -> ReviewDeps<'_> {
        (**self).deps()
    }
}
