//! The short form of a rule, written out as a tree-sitter query. A finding is a `@finding`
//! capture, so everything a rule finds is found by one engine.

use crate::domain::rule::MatchForm;

/// The query text for a match form.
///
/// Examples: `{ kind = "import_statement", regex = "sqlite3" }` is
/// `((import_statement) @finding (#match? @finding "sqlite3"))`; a form with `not` adds
/// `(#not-match? @finding "…")`; `any` writes its forms one after another; a bare kind is
/// `(kind) @finding`.
pub fn query_text(form: &MatchForm) -> String {
    let mut patterns = Vec::new();
    collect_patterns(form, &mut patterns);
    patterns.join("\n")
}

fn collect_patterns(form: &MatchForm, patterns: &mut Vec<String>) {
    if form.kind.is_some() || form.regex.is_some() || form.not.is_some() {
        patterns.push(pattern_of(form));
    }
    for inner in &form.any {
        collect_patterns(inner, patterns);
    }
}

fn pattern_of(form: &MatchForm) -> String {
    let node = match &form.kind {
        Some(kind) => format!("({kind}) @finding"),
        None => "(_) @finding".to_string(),
    };
    let mut predicates = Vec::new();
    if let Some(regex) = &form.regex {
        predicates.push(format!("(#match? @finding \"{}\")", escaped(regex)));
    }
    if let Some(regex) = &form.not {
        predicates.push(format!("(#not-match? @finding \"{}\")", escaped(regex)));
    }
    if predicates.is_empty() {
        node
    } else {
        format!("({node} {})", predicates.join(" "))
    }
}

/// A regular expression as the text of a query string: `\` and `"` are escaped, so the engine
/// reads back exactly the expression the person wrote.
fn escaped(regex: &str) -> String {
    regex.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(kind: Option<&str>, regex: Option<&str>) -> MatchForm {
        MatchForm {
            kind: kind.map(str::to_string),
            regex: regex.map(str::to_string),
            ..MatchForm::default()
        }
    }

    #[test]
    fn the_short_form_compiles_to_query_text() {
        assert_eq!(
            query_text(&form(Some("import_statement"), Some("sqlite3"))),
            "((import_statement) @finding (#match? @finding \"sqlite3\"))"
        );
        assert_eq!(
            query_text(&form(Some("raise_statement"), None)),
            "(raise_statement) @finding"
        );
        assert_eq!(
            query_text(&form(None, Some("x"))),
            "((_) @finding (#match? @finding \"x\"))"
        );
    }

    #[test]
    fn not_and_any_are_written_out() {
        let mut raise = form(Some("raise_statement"), Some("^raise [A-Z]"));
        raise.not = Some("Error\\(".into());
        assert_eq!(
            query_text(&raise),
            "((raise_statement) @finding (#match? @finding \"^raise [A-Z]\") (#not-match? @finding \"Error\\\\(\"))"
        );
        let any = MatchForm {
            any: vec![form(Some("a"), None), form(Some("b"), Some("c"))],
            ..MatchForm::default()
        };
        assert_eq!(
            query_text(&any),
            "(a) @finding\n((b) @finding (#match? @finding \"c\"))"
        );
    }

    #[test]
    fn a_regex_with_a_backslash_and_a_quote_is_escaped() {
        assert_eq!(
            query_text(&form(Some("call"), Some("\\.unwrap\\(\\)$\""))),
            "((call) @finding (#match? @finding \"\\\\.unwrap\\\\(\\\\)$\\\"\"))"
        );
    }
}
