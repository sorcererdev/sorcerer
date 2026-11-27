//! Doubles for the rule checker: one that cannot check, and one that forbids a word.

use std::path::PathBuf;
use std::sync::Mutex;

use std::path::Path;

use conjure::domain::spellbook_findings::{RuleFinding, RuleNote, RulesRead};
use conjure::ports::rule_checker::RuleChecker;
use sorcerer_core::domain::paths::forward_slashes;

/// A checker that can say nothing, as in a project without spellbook.
pub struct NotChecking;

impl RuleChecker for NotChecking {
    fn findings_in(&self, _: &[(PathBuf, String)]) -> RulesRead {
        RulesRead::NotChecked("no checker in this test".into())
    }

    fn describe(&self) -> String {
        "not checked: no checker in this test".into()
    }
}

/// A rule that finds every line holding a word, at the given severity; it keeps each set of
/// texts it was asked about.
pub struct ForbiddenWord {
    pub word: String,
    pub rule: String,
    pub severity: String,
    pub asked: Mutex<Vec<Vec<(PathBuf, String)>>>,
    /// What `rules_for` answers: the rules said to apply to every file.
    pub notes: Vec<RuleNote>,
}

impl ForbiddenWord {
    pub fn error(word: &str, rule: &str) -> ForbiddenWord {
        ForbiddenWord {
            word: word.into(),
            rule: rule.into(),
            severity: "error".into(),
            asked: Mutex::new(Vec::new()),
            notes: Vec::new(),
        }
    }

    /// The same rule, also told to the writer up front.
    pub fn told_up_front(mut self, message: &str, why: &str) -> ForbiddenWord {
        self.notes.push(RuleNote {
            id: self.rule.clone(),
            severity: self.severity.clone(),
            message: message.into(),
            why: Some(why.into()),
        });
        self
    }

    pub fn times_asked(&self) -> usize {
        self.asked.lock().unwrap().len()
    }
}

impl RuleChecker for ForbiddenWord {
    fn findings_in(&self, texts: &[(PathBuf, String)]) -> RulesRead {
        self.asked.lock().unwrap().push(texts.to_vec());
        let findings = texts
            .iter()
            .flat_map(|(path, text)| {
                text.lines()
                    .enumerate()
                    .filter(|(_, line)| line.contains(&self.word))
                    .map(|(at, line)| RuleFinding {
                        rule: self.rule.clone(),
                        file: forward_slashes(path),
                        line: at as u32 + 1,
                        severity: self.severity.clone(),
                        message: format!("No {} here", self.word),
                        fingerprint: line.trim().to_string(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        RulesRead::Checked(findings)
    }

    fn rules_for(&self, _: &Path) -> Vec<RuleNote> {
        self.notes.clone()
    }

    fn describe(&self) -> String {
        format!("forbids {}", self.word)
    }
}
