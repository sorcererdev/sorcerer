//! From a save to a pass: the settle wait, one pass at a time, a save that overtakes the
//! running review call, the progress line and the messages a pass may say.
//!
//! A save adds its file to the pending set and restarts the settle wait. When the wait ends
//! and no pass runs, a job thread takes the whole pending set and reviews it, the way `scry
//! <files>` would, within the editor's caps. A save during a pass stays pending; when the pass
//! ends with files pending, the next starts at once.
//!
//! Example: you save `orders.py` twice within two seconds. One pass starts two seconds after
//! the second save, sends `cancel` (the unit you changed), shows `scry: reviewing 1 unit`
//! while the call runs, and publishes the finding it opens.
//!
//! The text the server says and logs is built last in the file.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lsp_types::MessageType;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::view::CodebaseView;
use sorcerer_lsp::WorkDone;

use super::{Server, lock, project_file};
use crate::adapters::markdown_run_log::{RESULT, runs_dir, write_run};
use crate::adapters::scry_toml::ScryToml;
use crate::adapters::wiring::Wiring;
use crate::domain::budget::Caps;
use crate::domain::config::ScryConfig;
use crate::domain::log_text::{count, section, usd};
use crate::domain::scope::ScopeRequest;
use crate::ports::settings::Settings;
use crate::usecases::review_change::scope::scope_of;
use crate::usecases::review_change::{
    Progress, ReviewDeps, ReviewOutcome, ReviewReport, ReviewRequest, SentUnit, review_change,
};

/// The files saved and not yet reviewed, and the pass running, if one is.
#[derive(Default)]
pub(super) struct Passes {
    pending: BTreeSet<PathBuf>,
    running: Option<Arc<RunningPass>>,
    shut_down: bool,
}

/// A pass while it runs: its files, its two cancels, what its last review call was sent and
/// its progress line.
struct RunningPass {
    files: Vec<PathBuf>,
    /// Stops its review calls: set when a save overtakes the call in flight, or at shutdown.
    review_cancel: Cancel,
    /// Stops Jev and the claim checks: set only at shutdown.
    rest: Cancel,
    /// The units the last review call was sent, at the fingerprints they were sent at.
    sent: Mutex<Vec<SentUnit>>,
    /// A save overtook its review call and cancelled it.
    overtaken: AtomicBool,
    line: Mutex<Option<WorkDone>>,
}

impl RunningPass {
    fn made_a_call(&self) -> bool {
        !lock(&self.sent).is_empty() || lock(&self.line).is_some()
    }
}

/// The key the settle wait and the passes run under.
const PASS: &str = "pass";
/// The key the overtaking check runs under.
const OVERTAKE: &str = "overtake";

impl<W: Wiring + 'static> Server<W> {
    /// A file was saved: it waits for the next pass, the settle wait starts again, and if a
    /// pass is running, the save may overtake its review call.
    pub(super) fn saved(self: &Arc<Self>, file: PathBuf) {
        let running = {
            let mut passes = lock(&self.passes);
            if passes.shut_down {
                return;
            }
            passes.pending.insert(file.clone());
            passes.running.clone()
        };
        if let Some(pass) = running {
            self.check_overtaken_after(&pass, &file);
        }
        self.start_pass_after(self.cfg.settle);
    }

    /// A file to review at once, with no settle wait (after a dismissal).
    pub(super) fn queue(self: &Arc<Self>, file: PathBuf) {
        {
            let mut passes = lock(&self.passes);
            if passes.shut_down {
                return;
            }
            passes.pending.insert(file);
        }
        self.start_pass_after(Duration::ZERO);
    }

    /// The editor is shutting down: the running pass is stopped, nothing pending is
    /// reviewed, and no pass starts again.
    pub(super) fn shutdown(&self) {
        let mut passes = lock(&self.passes);
        passes.shut_down = true;
        passes.pending.clear();
        if let Some(pass) = &passes.running {
            pass.review_cancel.cancel();
            pass.rest.cancel();
        }
        self.jobs.cancel(PASS);
    }

    fn start_pass_after(self: &Arc<Self>, wait: Duration) {
        let server = Arc::clone(self);
        self.jobs.submit(PASS, wait, move || server.run_pending());
    }

    /// Runs one pass over the files pending, unless a pass is already running (it starts the
    /// next when it ends). Whatever happens inside the pass, an error or a panic included,
    /// the pass ends cleanly: its progress line ends, it is no longer running, and if files
    /// are pending the next pass starts.
    fn run_pending(self: &Arc<Self>) {
        let Some(pass) = self.take_pending() else {
            return;
        };
        let ended = catch_unwind(AssertUnwindSafe(|| self.review(&pass)));
        let failure = match ended {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(format!("{e:#}")),
            Err(panic) => Some(panic_text(panic.as_ref())),
        };
        if let Some(why) = &failure {
            self.log_error(why);
        }
        end_line(&pass, failure.is_some());
        self.finish(&pass);
    }

    /// The pending files as a new running pass; `None` when a pass is running, nothing is
    /// pending, or the server is shutting down.
    fn take_pending(&self) -> Option<Arc<RunningPass>> {
        let mut passes = lock(&self.passes);
        if passes.running.is_some() || passes.pending.is_empty() || passes.shut_down {
            return None;
        }
        let pass = Arc::new(RunningPass {
            files: std::mem::take(&mut passes.pending).into_iter().collect(),
            review_cancel: Cancel::default(),
            rest: Cancel::default(),
            sent: Mutex::new(Vec::new()),
            overtaken: AtomicBool::new(false),
            line: Mutex::new(None),
        });
        passes.running = Some(Arc::clone(&pass));
        Some(pass)
    }

    /// The pass is over: the next starts at once when files were saved meanwhile.
    fn finish(self: &Arc<Self>, pass: &Arc<RunningPass>) {
        let more = {
            let mut passes = lock(&self.passes);
            if passes
                .running
                .as_ref()
                .is_some_and(|running| Arc::ptr_eq(running, pass))
            {
                passes.running = None;
            }
            !passes.pending.is_empty()
        };
        if more {
            self.start_pass_after(Duration::ZERO);
        }
    }

    /// One pass:
    ///
    /// 1. opens the project and keeps the saved files that are its own; a project that needs
    ///    trust and does not have it is not reviewed (the gate may ask);
    /// 2. builds the wiring with the pass's two cancels; when it cannot be built, says once
    ///    that scry cannot run, if the files have something to review;
    /// 3. reviews the files' changed units within the editor's caps (`budget_per_pass`, the
    ///    day's `budget_per_day`), telling the progress line what each call is sent and
    ///    republishing whatever the pass stores;
    /// 4. republishes, ends the progress line, says when the day's budget is spent, and keeps
    ///    a run log when it made a call or failed.
    fn review(self: &Arc<Self>, pass: &RunningPass) -> anyhow::Result<()> {
        let project = (self.cfg.open)(&self.root)?;
        let files: Vec<PathBuf> = pass
            .files
            .iter()
            .filter_map(|file| project_file(&project, file))
            .collect();
        if files.is_empty() || !self.trusted_at_save(&project, &pass.files) {
            return Ok(());
        }
        let config = ScryToml::for_project(&project).scry_config()?;
        let wired = match (self.cfg.wire)(&project, pass.review_cancel.clone(), pass.rest.clone()) {
            Ok(wired) => wired,
            Err(e) => {
                self.cannot_run(&project, &files, &config, &e);
                return Ok(());
            }
        };
        let run_log = Mutex::new(String::new());
        let log = |text: &str| lock(&run_log).push_str(text);
        let progress = |p: &Progress| self.told(pass, p);
        let deps = ReviewDeps {
            log: &log,
            progress: &progress,
            ..wired.deps()
        };
        let now = deps.clock.now();
        let outcome = review_change(&project, &editor_request(files.clone(), &config), &deps);
        let report = match outcome {
            Ok(ReviewOutcome::Ran(report)) => report,
            Ok(ReviewOutcome::Skipped(why)) => {
                self.client.log(&skipped_text(&why));
                return Ok(());
            }
            Err(e) => {
                self.keep_run_log(&project, now, &failed_run_text(&files, &lock(&run_log), &e));
                return Err(e.into());
            }
        };
        self.after_review(pass, &config, &report);
        if pass.made_a_call() || !report.failures.is_empty() {
            let text = run_text(&files, &lock(&run_log), &report);
            self.keep_run_log(&project, now, &text);
        }
        Ok(())
    }

    /// What the engine tells the pass as it goes: a call about to go out starts (or updates)
    /// the progress line and is remembered for the overtaking check; a write of the store is
    /// republished, once the pass has made a call (a pass that sends nothing stores nothing
    /// new to show).
    fn told(&self, pass: &RunningPass, progress: &Progress) {
        match progress {
            Progress::Sending(units) => {
                *lock(&pass.sent) = units.clone();
                let message = reviewing_text(units.len());
                let mut line = lock(&pass.line);
                match line.as_ref() {
                    Some(line) => line.report(&message),
                    None => *line = Some(self.client.work_done("scry", &message)),
                }
            }
            Progress::Stored if pass.made_a_call() => self.publish_open(),
            Progress::Stored | Progress::Reviewed(_) => {}
        }
    }

    /// After the review: what it changed is published, the progress line ends (saying the
    /// review failed, unless a save cancelled it on purpose), and the day's message is said
    /// when the day's cap held something back.
    fn after_review(&self, pass: &RunningPass, config: &ScryConfig, report: &ReviewReport) {
        if pass.made_a_call() || !report.touched.is_empty() {
            self.publish_open();
        }
        let failed = !report.failures.is_empty() && !pass.overtaken.load(Ordering::SeqCst);
        end_line(pass, failed);
        if report.day_spent {
            let local_day = self
                .cfg
                .clock
                .now()
                .with_timezone(&self.cfg.clock.offset())
                .date_naive();
            self.client.say_once(
                &format!("day-{local_day}"),
                MessageType::WARNING,
                &day_spent_text(config.budget_per_day),
            );
        }
    }

    /// The wiring could not be built (no Jev key, no provider): logged always, and said once
    /// a session when the saved files hold a unit that differs from the base, so a save with
    /// nothing to review stays silent.
    fn cannot_run(
        &self,
        project: &Project,
        files: &[PathBuf],
        config: &ScryConfig,
        e: &anyhow::Error,
    ) {
        let reason = format!("{e:#}");
        self.client.log(&cannot_run_text(&reason));
        let view = FilesView::new(Arc::new(project.clone()), self.cfg.cache.clone());
        let to_review = scope_of(
            project,
            &view,
            ScopeRequest::Files(files.to_vec()),
            false,
            config,
        )
        .is_ok_and(|scope| !scope.units.is_empty());
        if to_review {
            self.client.say_once(
                "cannot-run",
                MessageType::WARNING,
                &cannot_run_text(&reason),
            );
        }
    }

    /// Writes the pass's run log under the project's state directory; a failure to write it
    /// is logged.
    fn keep_run_log(&self, project: &Project, now: chrono::DateTime<chrono::Utc>, text: &str) {
        let written = project
            .state_dir()
            .map_err(anyhow::Error::from)
            .and_then(|state| Ok(write_run(&runs_dir(&state), now, text)?));
        if let Err(e) = written {
            self.client
                .log(&format!("scry: the run log was not written: {e:#}"));
        }
    }

    /// A save during a pass, of a file its review call was sent units of, is checked on a
    /// thread of its own: when none of the units the call was sent still has its
    /// fingerprint, the call is cancelled, so the fresh text is reviewed sooner. If any still
    /// holds, the call runs on (the engine drops what it says about units that moved).
    fn check_overtaken_after(self: &Arc<Self>, pass: &Arc<RunningPass>, saved: &Path) {
        let server = Arc::clone(self);
        let pass = Arc::clone(pass);
        let saved = saved.to_path_buf();
        self.jobs.submit(OVERTAKE, Duration::ZERO, move || {
            if server.overtakes(&pass, &saved) {
                pass.overtaken.store(true, Ordering::SeqCst);
                pass.review_cancel.cancel();
            }
        });
    }

    /// Whether the saved file holds a unit the call was sent, and none of the units it was
    /// sent still has the fingerprint it was sent at.
    fn overtakes(&self, pass: &RunningPass, saved: &Path) -> bool {
        let sent = lock(&pass.sent).clone();
        let Ok(project) = (self.cfg.open)(&self.root) else {
            return false;
        };
        let Some(saved) = project_file(&project, saved) else {
            return false;
        };
        if !self.trusted(&project) {
            return false;
        }
        if !sent.iter().any(|unit| unit.file == saved) {
            return false;
        }
        let view = FilesView::new(Arc::new(project), self.cfg.cache.clone());
        !sent.iter().any(|unit| still_as_sent(&view, unit))
    }
}

/// Whether the unit is in its file now at the fingerprint it was sent at.
fn still_as_sent(view: &FilesView, unit: &SentUnit) -> bool {
    view.units(&unit.file).is_ok_and(|units| {
        units
            .iter()
            .any(|u| u.name == unit.unit && u.fingerprint == unit.fingerprint)
    })
}

/// The editor's pass over the saved files: their changed units, not sent again when already
/// reviewed, within `budget_per_pass` and the day's `budget_per_day`.
fn editor_request(files: Vec<PathBuf>, config: &ScryConfig) -> ReviewRequest {
    ReviewRequest {
        scope: ScopeRequest::Files(files),
        all: false,
        fresh: false,
        budget: None,
        caps: Some(Caps {
            pass: Some(config.budget_per_pass),
            day: config.budget_per_day,
            budget: None,
        }),
    }
}

/// Ends the pass's progress line if it began, saying the review failed when it did.
fn end_line(pass: &RunningPass, failed: bool) {
    if let Some(line) = lock(&pass.line).take() {
        line.end(failed.then_some(REVIEW_FAILED));
    }
}

// The text the server says and logs.

/// How a progress line ends when a review call failed.
const REVIEW_FAILED: &str = "review failed, see \"scry log\"";

/// `reviewing 1 unit`, `reviewing 2 units`.
fn reviewing_text(units: usize) -> String {
    format!("reviewing {}", count(units, "unit"))
}

/// The day's message, with the day's budget: `scry: today's budget ($3.00) is spent. …`.
fn day_spent_text(budget_per_day: f64) -> String {
    format!(
        "scry: today's budget (${budget_per_day:.2}) is spent. Reviews resume tomorrow; \"scry cost\" shows the days."
    )
}

fn cannot_run_text(reason: &str) -> String {
    format!("scry is not reviewing: {reason}. Run \"scry doctor\".")
}

fn skipped_text(why: &str) -> String {
    format!("scry: a pass was skipped: {why}")
}

/// What a panic carried, as text.
fn panic_text(panic: &(dyn std::any::Any + Send)) -> String {
    let what = panic
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".into());
    format!("it panicked: {what}")
}

/// The run log of an editor's pass: what was saved, what the engine logged, and the result.
fn run_text(files: &[PathBuf], engine_log: &str, report: &ReviewReport) -> String {
    format!(
        "{}{engine_log}\n{RESULT}{} looked at, {} new, {}, {} failed (editor)\n",
        head_text(files),
        count(report.looked_at, "unit"),
        count(report.new.len(), "finding"),
        usd(report.spent),
        count(report.failures.len(), "call"),
    )
}

/// The run log of an editor's pass that could not finish.
fn failed_run_text(files: &[PathBuf], engine_log: &str, e: &sorcerer_core::Error) -> String {
    format!(
        "{}{engine_log}\n- error: {e}\n\n{RESULT}could not run (editor)\n",
        head_text(files)
    )
}

fn head_text(files: &[PathBuf]) -> String {
    let saved: Vec<String> = files.iter().map(|f| format!("`{}`", f.display())).collect();
    format!(
        "# scry lsp: a pass after a save\n{}- saved: {}\n",
        section("Run"),
        saved.join(", ")
    )
}
