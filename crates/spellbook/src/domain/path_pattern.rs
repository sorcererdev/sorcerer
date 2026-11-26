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
    /// Example: `src/{app}/{service}/commands/*.txt` matches `src/bistro/booking/commands/a.txt`
    /// and binds `app` to `bistro` and `service` to `booking`; `src/{app}/web/**.txt` is
    /// `src/{app}/web/**/*.txt`, so it matches `src/bistro/web/bookings/routes.txt` too.
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

    /// The names of its `{slot}`s, in the pattern's order.
    ///
    /// Example: `src/{app}/{service}/*.txt` has the slots `app` and `service`.
    pub fn slots(&self) -> &[String] {
        &self.slots
    }

    /// Whether the path matches, and the value each `{slot}` took when it does.
    ///
    /// Example: `src/{app}/main.txt` on `src/bistro/main.txt` is `Some({"app": "bistro"})`.
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
/// anything below; `**.txt` is `**/*.txt`. Returns what is left of the pattern.
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
                "src/{app}/{service}/commands/*.txt",
                "src/bistro/booking/commands/a.txt"
            ),
            Some(vec![pair("app", "bistro"), pair("service", "booking")])
        );
        assert_eq!(
            bound(
                "src/{app}/{service}/commands/*.txt",
                "src/bistro/booking/queries/a.txt"
            ),
            None
        );
    }

    #[test]
    fn a_star_stays_in_one_name() {
        assert!(bound("src/*.txt", "src/a.txt").is_some());
        assert!(bound("src/*.txt", "src/b/a.txt").is_none());
    }

    #[test]
    fn double_star_is_any_folders_and_none() {
        assert!(bound("**/__init__.txt", "__init__.txt").is_some());
        assert!(bound("**/__init__.txt", "src/a/b/__init__.txt").is_some());
        assert!(bound("**/ports/**", "src/bistro/booking/ports/x/y.txt").is_some());
        assert!(bound("**/ports/**", "src/bistro/booking/commands/y.txt").is_none());
        assert!(bound("**/*__test.txt", "src/a/b__test.txt").is_some());
    }

    #[test]
    fn double_star_before_a_suffix_is_olifants_short_form() {
        assert_eq!(
            bound("src/{app}/web/**.txt", "src/bistro/web/bookings/routes.txt"),
            Some(vec![pair("app", "bistro")])
        );
        assert!(bound("src/{app}/web/**.txt", "src/bistro/web/app.txt").is_some());
        assert!(bound("src/{app}/web/**.txt", "src/bistro/web/home.html").is_none());
    }

    #[test]
    fn a_slot_binds_the_part_between_literals() {
        assert_eq!(
            bound(
                "src/{service}/commands/{command}.md",
                "src/booking/commands/cancel_table.md"
            ),
            Some(vec![
                pair("command", "cancel_table"),
                pair("service", "booking")
            ])
        );
    }

    #[test]
    fn the_dot_is_a_dot() {
        assert!(bound("a.txt", "aXtxt").is_none());
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
