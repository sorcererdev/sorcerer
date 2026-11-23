//! A path pattern: the `files` and `exclude` of a rule. Written the way olifant's rules were:
//! `*` is part of one folder or file name, `**` is any number of folders, and `{slot}` binds
//! one name (or the part of a name between the literal text around it).

use std::collections::BTreeMap;

use regex::Regex;

/// A compiled path pattern over `/`-separated paths relative to the project root.
#[derive(Debug, Clone)]
pub struct PathPattern {
    text: String,
    regex: Regex,
    slots: Vec<String>,
}

impl PathPattern {
    /// Compiles a pattern.
    ///
    /// Example: `src/{app}/{service}/commands/*.py` matches `src/bistro/booking/commands/a.py`
    /// and binds `app` to `bistro` and `service` to `booking`; `src/{app}/web/**.py` is
    /// `src/{app}/web/**/*.py`, so it matches `src/bistro/web/bookings/routes.py` too.
    pub fn parse(text: &str) -> Result<PathPattern, String> {
        let mut regex = String::from("^");
        let mut slots: Vec<String> = Vec::new();
        let mut rest = text;
        while let Some(next) = rest.chars().next() {
            rest = match next {
                '*' if rest.starts_with("**") => any_folders(&rest[2..], &mut regex),
                '*' => literal_then(&rest[1..], "[^/]*", &mut regex),
                '?' => literal_then(&rest[1..], "[^/]", &mut regex),
                '{' => slot(rest, &mut regex, &mut slots, text)?,
                other => {
                    regex.push_str(&regex::escape(&other.to_string()));
                    &rest[other.len_utf8()..]
                }
            };
        }
        regex.push('$');
        let compiled =
            Regex::new(&regex).map_err(|e| format!("`{text}` is not a path pattern: {e}"))?;
        Ok(PathPattern {
            text: text.to_string(),
            regex: compiled,
            slots,
        })
    }

    /// The pattern as written.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the path matches, and the value each `{slot}` took when it does.
    ///
    /// Example: `src/{app}/main.py` on `src/bistro/main.py` is `Some({"app": "bistro"})`.
    pub fn matches(&self, path: &str) -> Option<BTreeMap<String, String>> {
        let found = self.regex.captures(path)?;
        Some(
            self.slots
                .iter()
                .filter_map(|name| Some((name.clone(), found.name(name)?.as_str().to_string())))
                .collect(),
        )
    }
}

/// `**`: any folders (none too). `**/` is folders then a slash, and `**` ending the pattern is
/// anything below; `**.py` is `**/*.py`. Returns what is left of the pattern.
fn any_folders<'p>(after: &'p str, regex: &mut String) -> &'p str {
    if let Some(rest) = after.strip_prefix('/') {
        regex.push_str("(?:.+/)?");
        return rest;
    }
    if after.is_empty() {
        regex.push_str(".*");
        return after;
    }
    regex.push_str("(?:.+/)?[^/]*");
    after
}

fn literal_then<'p>(after: &'p str, piece: &str, regex: &mut String) -> &'p str {
    regex.push_str(piece);
    after
}

/// `{name}`: the part of a name the slot binds. Returns what is left of the pattern.
fn slot<'p>(
    rest: &'p str,
    regex: &mut String,
    slots: &mut Vec<String>,
    whole: &str,
) -> Result<&'p str, String> {
    let close = rest
        .find('}')
        .ok_or_else(|| format!("`{whole}`: a `{{` is never closed"))?;
    let name = rest[1..close].trim();
    let is_a_name = !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !is_a_name {
        return Err(format!("`{whole}`: `{{{name}}}` is not a slot name"));
    }
    if slots.iter().any(|known| known == name) {
        return Err(format!("`{whole}`: the slot `{{{name}}}` is used twice"));
    }
    regex.push_str(&format!("(?P<{name}>[^/]+?)"));
    slots.push(name.to_string());
    Ok(&rest[close + 1..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound(pattern: &str, path: &str) -> Option<Vec<(String, String)>> {
        PathPattern::parse(pattern)
            .unwrap()
            .matches(path)
            .map(|slots| slots.into_iter().collect())
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_string(), value.to_string())
    }

    #[test]
    fn slots_bind_one_folder_each() {
        assert_eq!(
            bound(
                "src/{app}/{service}/commands/*.py",
                "src/bistro/booking/commands/a.py"
            ),
            Some(vec![pair("app", "bistro"), pair("service", "booking")])
        );
        assert_eq!(
            bound(
                "src/{app}/{service}/commands/*.py",
                "src/bistro/booking/queries/a.py"
            ),
            None
        );
    }

    #[test]
    fn a_star_stays_in_one_name() {
        assert!(bound("src/*.py", "src/a.py").is_some());
        assert!(bound("src/*.py", "src/b/a.py").is_none());
    }

    #[test]
    fn double_star_is_any_folders_and_none() {
        assert!(bound("**/__init__.py", "__init__.py").is_some());
        assert!(bound("**/__init__.py", "src/a/b/__init__.py").is_some());
        assert!(bound("**/ports/**", "src/bistro/booking/ports/x/y.py").is_some());
        assert!(bound("**/ports/**", "src/bistro/booking/commands/y.py").is_none());
        assert!(bound("**/*__test.py", "src/a/b__test.py").is_some());
    }

    #[test]
    fn double_star_before_a_suffix_is_olifants_short_form() {
        assert_eq!(
            bound("src/{app}/web/**.py", "src/bistro/web/bookings/routes.py"),
            Some(vec![pair("app", "bistro")])
        );
        assert!(bound("src/{app}/web/**.py", "src/bistro/web/app.py").is_some());
        assert!(bound("src/{app}/web/**.py", "src/bistro/web/home.html").is_none());
    }

    #[test]
    fn a_slot_binds_the_part_between_literals() {
        assert_eq!(
            bound(
                "src/{service}/commands/{command}.rs",
                "src/booking/commands/cancel_table.rs"
            ),
            Some(vec![
                pair("command", "cancel_table"),
                pair("service", "booking")
            ])
        );
    }

    #[test]
    fn the_dot_is_a_dot() {
        assert!(bound("a.py", "aXpy").is_none());
    }

    #[test]
    fn a_broken_pattern_says_so() {
        assert!(
            PathPattern::parse("src/{app")
                .unwrap_err()
                .contains("never closed")
        );
        assert!(
            PathPattern::parse("src/{}/x")
                .unwrap_err()
                .contains("not a slot name")
        );
        assert!(
            PathPattern::parse("{a}/{a}")
                .unwrap_err()
                .contains("used twice")
        );
    }
}
