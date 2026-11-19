//! Steering: the shipped text, then the project's `.sorcerer/scry.md`. All of
//! it goes to the finder; each heading under `## Rules` is a rule a finding
//! can cite; `## A nitpick is` and `## Worth saying is` replace the two sides
//! of Jev's nitpick question.

use sorcerer_core::{Error, Project, Result};

const SHIPPED: &str = include_str!("../../../library/scry.md");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Steering {
    pub text: String,
    /// (name, text) in file order; a duplicate name keeps both, the first wins.
    pub rules: Vec<(String, String)>,
    pub nitpick: String,
    pub worth: String,
}

/// A heading line's level and title (`### A` is (3, "A")).
fn heading(l: &str) -> Option<(usize, &str)> {
    let rest = l.trim_start_matches('#');
    let n = l.len() - rest.len();
    (n > 0 && (rest.is_empty() || rest.starts_with(char::is_whitespace))).then(|| (n, rest.trim()))
}

/// Each heading with the lines under it, up to the next heading of any level.
fn sections(src: &str) -> Vec<((usize, &str), String)> {
    let mut out: Vec<((usize, &str), Vec<&str>)> = Vec::new();
    for l in src.lines() {
        match heading(l) {
            Some(h) => out.push((h, Vec::new())),
            None => {
                if let Some((_, b)) = out.last_mut() {
                    b.push(l);
                }
            }
        }
    }
    out.into_iter()
        .map(|(h, b)| (h, b.join("\n").trim().to_string()))
        .collect()
}

/// The shipped sides, read from the shipped file.
fn shipped_sides() -> (String, String) {
    let ss = sections(SHIPPED);
    let side = |name: &str| {
        ss.iter()
            .find(|((l, n), _)| *l == 2 && *n == name)
            .map(|(_, b)| b.clone())
            .unwrap_or_default()
    };
    (side("A nitpick is"), side("Worth saying is"))
}

/// The steering of TEXTS, layers joined by a blank line. The last non-empty
/// `## A nitpick is` / `## Worth saying is` wins, else the shipped sides.
pub fn steering_of(texts: &[&str]) -> Steering {
    let text = texts.join("\n\n");
    let ss = sections(&text);
    let (ship_nit, ship_worth) = shipped_sides();
    let last = |name: &str, default: String| {
        ss.iter()
            .rev()
            .find(|((l, n), b)| *l == 2 && *n == name && !b.is_empty())
            .map(|(_, b)| b.clone())
            .unwrap_or(default)
    };
    let rules_at = ss.iter().position(|((l, n), _)| *l == 2 && *n == "Rules");
    let rules = match rules_at {
        None => Vec::new(),
        Some(i) => ss[i + 1..]
            .iter()
            .take_while(|((l, _), _)| *l > 2)
            .map(|((_, n), b)| (n.to_string(), b.clone()))
            .collect(),
    };
    Steering {
        nitpick: last("A nitpick is", ship_nit),
        worth: last("Worth saying is", ship_worth),
        rules,
        text,
    }
}

/// The shipped layer, then `.sorcerer/scry.md` when the project has one.
pub fn steering(p: &Project) -> Result<Steering> {
    let file = p.root.join(".sorcerer/scry.md");
    match std::fs::read_to_string(&file) {
        Ok(t) => Ok(steering_of(&[SHIPPED, &t])),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(steering_of(&[SHIPPED])),
        Err(e) => Err(Error::io(&file, e)),
    }
}

impl Steering {
    /// The heading a cited rule names: ignoring case, the cited text inside
    /// the heading or the heading inside it; the first in file order wins.
    pub fn resolve_rule(&self, cited: &str) -> Option<&str> {
        let c = cited.trim().to_lowercase();
        if c.is_empty() {
            return None;
        }
        self.rules.iter().map(|(n, _)| n.as_str()).find(|n| {
            let h = n.to_lowercase();
            h.contains(&c) || c.contains(&h)
        })
    }

    pub fn rule_text(&self, name: &str) -> Option<&str> {
        self.rules
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.as_str())
    }

    /// `## Rules` and everything under it, up to the next `##` or `#`
    /// heading; empty without one.
    pub fn rules_section(&self) -> String {
        let mut out: Vec<&str> = Vec::new();
        let mut inside = false;
        for l in self.text.lines() {
            match heading(l) {
                Some((2, "Rules")) if !inside => inside = true,
                Some((n, _)) if inside && n <= 2 => break,
                _ => {}
            }
            if inside {
                out.push(l);
            }
        }
        out.join("\n").trim_end().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "## Rules\n\n### Commands return a Result (must)\nA command never panics.\n\n## Other\n\n### Not a rule\nx\n";

    #[test]
    fn rules_are_level_three_headings_under_rules() {
        let s = steering_of(&[SAMPLE]);
        assert_eq!(
            s.rules,
            [(
                "Commands return a Result (must)".to_string(),
                "A command never panics.".to_string()
            )]
        );
        assert_eq!(
            s.rule_text("Commands return a Result (must)"),
            Some("A command never panics.")
        );
        assert!(s.rules_section().starts_with("## Rules"));
        assert!(!s.rules_section().contains("Other"));
    }

    #[test]
    fn the_last_nitpick_section_wins() {
        let s = steering_of(&[
            "## A nitpick is\nfirst\n\n## Worth saying is\nw1",
            "## A nitpick is\nsecond",
        ]);
        assert_eq!(s.nitpick, "second");
        assert_eq!(s.worth, "w1");
    }

    #[test]
    fn with_no_nitpick_section_the_shipped_sides_apply() {
        let s = steering_of(&["## Rules\n\n### R\nbody"]);
        assert_eq!(
            s.nitpick,
            "it is about taste or polish: naming, wording, formatting, style, a small optional refactor, or speculative future-proofing; nothing would go wrong if it were ignored"
        );
        assert_eq!(
            s.worth,
            "ignoring it leaves wrong behaviour, a failure, lost data, a security hole, or a real maintenance cost"
        );
        assert_eq!(steering_of(&[SHIPPED]).nitpick, s.nitpick);
    }

    #[test]
    fn resolve_rule_matches_either_way_ignoring_case() {
        let s = steering_of(&[SAMPLE]);
        let name = Some("Commands return a Result (must)");
        assert_eq!(s.resolve_rule("commands return a result"), name);
        assert_eq!(
            s.resolve_rule("### Commands return a Result (must) — handlers"),
            name
        );
        assert_eq!(s.resolve_rule(""), None);
        assert_eq!(s.resolve_rule("nothing like it"), None);
    }

    #[test]
    fn the_project_file_is_a_second_layer() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".sorcerer")).unwrap();
        std::fs::write(d.path().join(".sorcerer/scry.md"), SAMPLE).unwrap();
        let p = Project::open(d.path()).unwrap();
        assert_eq!(steering(&p).unwrap().rules.len(), 1);
    }
}
