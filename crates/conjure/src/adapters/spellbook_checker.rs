//! The live rule checker: the `spellbook` program, asked to check texts that are not on disk
//! (`spellbook check --texts - --json`, the texts on its standard input). conjure does not link
//! spellbook (products never import each other); it runs the program the person installed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::json;
use sorcerer_core::domain::paths::forward_slashes;
use sorcerer_core::domain::project::Project;
use sorcerer_core::ports::env::Env;

use crate::domain::spellbook_findings::{
    RuleFinding, RuleNote, RulesRead, findings_of_check_json, rules_of_rules_json,
};
use crate::ports::rule_checker::RuleChecker;

/// The file that holds a project's checked rules; without it there is nothing to check.
pub const SPELLBOOK_RULES: &str = ".sorcerer/spellbook/rules.toml";

/// `spellbook`, run in the project's root; or why changes cannot be checked here.
pub struct SpellbookChecker {
    root: PathBuf,
    program: Result<PathBuf, String>,
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
            Ok(program) => match run_check(program, &self.root, texts) {
                Ok(findings) => RulesRead::Checked(findings),
                Err(why) => RulesRead::NotChecked(why),
            },
        }
    }

    fn rules_for(&self, file: &Path) -> Vec<RuleNote> {
        let Ok(program) = &self.program else {
            return Vec::new();
        };
        run_rules(program, &self.root, file).unwrap_or_default()
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

/// Runs `spellbook check --texts - --json` with the texts on its input. Exit 0 (the rules hold)
/// and 1 (an error found) both answer; anything else is spellbook's sentence on its error output.
fn run_check(
    program: &Path,
    root: &Path,
    texts: &[(PathBuf, String)],
) -> Result<Vec<RuleFinding>, String> {
    let asked = texts_json(texts);
    let mut child = Command::new(program)
        .args(["check", "--texts", "-", "--json"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spellbook did not start: {e}"))?;
    // written on its own thread, so a long input cannot block on a full output pipe
    let mut input = child.stdin.take();
    let writing = std::thread::spawn(move || {
        if let Some(input) = input.as_mut() {
            let _ = input.write_all(asked.as_bytes());
        }
    });
    let output = child
        .wait_with_output()
        .map_err(|e| format!("spellbook did not finish: {e}"))?;
    let _ = writing.join();
    match output.status.code() {
        Some(0 | 1) => findings_of_check_json(&String::from_utf8_lossy(&output.stdout)),
        _ => Err(first_line(&String::from_utf8_lossy(&output.stderr))),
    }
}

/// Runs `spellbook rules FILE --json`; exit 0 answers, anything else is spellbook's sentence.
fn run_rules(program: &Path, root: &Path, file: &Path) -> Result<Vec<RuleNote>, String> {
    let output = Command::new(program)
        .args(["rules", "--json"])
        .arg(forward_slashes(file))
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("spellbook did not start: {e}"))?;
    match output.status.code() {
        Some(0) => rules_of_rules_json(&String::from_utf8_lossy(&output.stdout)),
        _ => Err(first_line(&String::from_utf8_lossy(&output.stderr))),
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
