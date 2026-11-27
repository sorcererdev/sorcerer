//! The live rule checker: the `spellbook` program, asked to check texts that are not on disk
//! (`spellbook check --texts - --json`, the texts on its standard input). conjure does not link
//! spellbook (products never import each other); it runs the program the person installed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::json;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::domain::spellbook_findings::{
    CHECK_TIMEOUT, RuleFinding, RuleNote, RulesRead, findings_of_check_json, rules_of_rules_json,
};
use crate::ports::rule_checker::RuleChecker;

/// The file that holds a project's checked rules; without it there is nothing to check.
pub const SPELLBOOK_RULES: &str = ".sorcerer/spellbook/rules.toml";

/// `spellbook`, run in the project's root; or why changes cannot be checked here.
pub struct SpellbookChecker {
    root: PathBuf,
    program: Result<PathBuf, String>,
    timeout: Duration,
}

impl SpellbookChecker {
    /// The checker for a project: the `spellbook` beside this program (they are installed
    /// together), else the one on PATH. A project with no rules file is not checked, and no
    /// program is run for it.
    pub fn for_project(project: &Project, env: &dyn Env) -> SpellbookChecker {
        let program = if project.root.join(SPELLBOOK_RULES).is_file() {
            spellbook_beside_us()
                .or_else(|| env.which("spellbook"))
                .ok_or_else(|| "spellbook is not installed".to_string())
        } else {
            Err(format!("this project has no {SPELLBOOK_RULES}"))
        };
        SpellbookChecker {
            root: project.root.clone(),
            program,
            timeout: CHECK_TIMEOUT,
        }
    }

    /// The same checker with another wait before it gives up (for tests).
    pub fn with_timeout(mut self, timeout: Duration) -> SpellbookChecker {
        self.timeout = timeout;
        self
    }

    /// A checker that runs this program (for tests).
    pub fn running(root: PathBuf, program: PathBuf) -> SpellbookChecker {
        SpellbookChecker {
            root,
            program: Ok(program),
            timeout: CHECK_TIMEOUT,
        }
    }
}

impl RuleChecker for SpellbookChecker {
    fn findings_in(&self, texts: &[(PathBuf, String)]) -> RulesRead {
        if texts.is_empty() {
            return RulesRead::Checked(Vec::new());
        }
        match &self.program {
            Err(why) => RulesRead::NotChecked(why.clone()),
            Ok(program) => match run_check(program, &self.root, texts, self.timeout) {
                Ok(findings) => RulesRead::Checked(findings),
                Err(CheckStopped::TimedOut) => RulesRead::TimedOut,
                Err(CheckStopped::Because(why)) => RulesRead::NotChecked(why),
            },
        }
    }

    fn rules_for(&self, file: &Path) -> Vec<RuleNote> {
        let Ok(program) = &self.program else {
            return Vec::new();
        };
        run_rules(program, &self.root, file, self.timeout).unwrap_or_default()
    }

    fn describe(&self) -> String {
        match &self.program {
            Ok(program) => format!(
                "{}: changes are checked against {SPELLBOOK_RULES}",
                program.display()
            ),
            Err(why) => format!("not checked: {why}"),
        }
    }
}

/// The `spellbook` installed in the same folder as this program, when there is one.
fn spellbook_beside_us() -> Option<PathBuf> {
    let here = std::env::current_exe().ok()?;
    let beside = here
        .parent()?
        .join(format!("spellbook{}", std::env::consts::EXE_SUFFIX));
    beside.is_file().then_some(beside)
}

/// Why a check gave no findings.
enum CheckStopped {
    TimedOut,
    Because(String),
}

/// Runs `spellbook check --texts - --json` with the texts on its input, waiting at most
/// `timeout`. Exit 0 (the rules hold) and 1 (an error found) both answer; anything else is
/// spellbook's sentence on its error output.
fn run_check(
    program: &Path,
    root: &Path,
    texts: &[(PathBuf, String)],
    timeout: Duration,
) -> Result<Vec<RuleFinding>, CheckStopped> {
    let args = ["check", "--texts", "-", "--json"].map(String::from);
    let ran = run_within(program, root, &args, Some(texts_json(texts)), timeout)?;
    match ran.code {
        Some(0 | 1) => findings_of_check_json(&ran.stdout).map_err(CheckStopped::Because),
        _ => Err(CheckStopped::Because(first_line(&ran.stderr))),
    }
}

/// Runs `spellbook rules --json FILE`; exit 0 answers, anything else is spellbook's sentence.
fn run_rules(
    program: &Path,
    root: &Path,
    file: &Path,
    timeout: Duration,
) -> Result<Vec<RuleNote>, CheckStopped> {
    let args = ["rules".into(), "--json".into(), forward_slashes(file)];
    let ran = run_within(program, root, &args, None, timeout)?;
    match ran.code {
        Some(0) => rules_of_rules_json(&ran.stdout).map_err(CheckStopped::Because),
        _ => Err(CheckStopped::Because(first_line(&ran.stderr))),
    }
}

/// What a finished program left.
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Runs a program in the root with an optional text on its input, and waits at most `timeout`
/// for it; one still running then is stopped.
fn run_within(
    program: &Path,
    root: &Path,
    args: &[String],
    input: Option<String>,
    timeout: Duration,
) -> Result<Ran, CheckStopped> {
    let because = |why: String| CheckStopped::Because(why);
    let mut child = Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| because(format!("spellbook did not start: {e}")))?;
    // each pipe is served on its own thread, so a long input or output cannot block the wait
    let mut stdin = child.stdin.take();
    let writing = std::thread::spawn(move || {
        if let (Some(pipe), Some(text)) = (stdin.as_mut(), input) {
            let _ = pipe.write_all(text.as_bytes());
        }
    });
    let reading_out = std::thread::spawn(read_all(child.stdout.take()));
    let reading_err = std::thread::spawn(read_all(child.stderr.take()));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CheckStopped::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(because(format!("spellbook did not finish: {e}"))),
        }
    };
    let _ = writing.join();
    Ok(Ran {
        code: status.code(),
        stdout: reading_out.join().unwrap_or_default(),
        stderr: reading_err.join().unwrap_or_default(),
    })
}

/// Reads a pipe to its end as text; empty when there is none.
fn read_all<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
) -> impl FnOnce() -> String + Send + 'static {
    move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// `{"files": [{"path": "src/a.rs", "text": "…"}]}`, each path with `/`.
fn texts_json(texts: &[(PathBuf, String)]) -> String {
    let files: Vec<serde_json::Value> = texts
        .iter()
        .map(|(path, text)| json!({ "path": forward_slashes(path), "text": text }))
        .collect();
    json!({ "files": files }).to_string()
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        "spellbook stopped without saying why".to_string()
    } else {
        line.to_string()
    }
}
