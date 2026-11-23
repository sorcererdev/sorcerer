//! Templates: `{= name =}` marks a place a value goes, `{= name|pascal =}` writes the value in
//! another case style, and `{= ? name: kind | instruction =}` marks a hole a person (or a model)
//! fills later. Language files use the first form for paths; spellbook's generators use all
//! three.

use crate::domain::name_forms::apply_filter;

/// One piece of a template, in the order the template holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece<'t> {
    /// Text copied as it is.
    Text(&'t str),
    /// `{= name|filter|filter =}`: a value, its filters applied left to right.
    Value {
        name: &'t str,
        filters: Vec<&'t str>,
    },
    /// `{= ? name: kind | instruction =}`: a hole, what kind of code belongs there and what it
    /// is for. The instruction may be empty.
    Hole {
        name: &'t str,
        kind: &'t str,
        instruction: &'t str,
    },
}

/// Splits a template into its pieces. Errors name the trouble and quote the template: a `{=`
/// never closed, `{{ }}` (another template language), `{= if =}`/`{= for =}` (not yet), an
/// empty name, or a hole without its `: kind`.
///
/// Example: `pieces("a {= x|snake =}")` is `[Text("a "), Value { name: "x", filters: ["snake"] }]`.
pub fn pieces(template: &str) -> Result<Vec<Piece<'_>>, String> {
    let shown = excerpt(template);
    refuse_double_braces(template, &shown)?;
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find("{=") {
        if open > 0 {
            found.push(Piece::Text(&rest[..open]));
        }
        let after = &rest[open + 2..];
        let close = after
            .find("=}")
            .ok_or_else(|| format!("`{{=` is never closed in `{shown}`"))?;
        found.push(piece_inside(after[..close].trim(), &shown)?);
        rest = &after[close + 2..];
    }
    if !rest.is_empty() {
        found.push(Piece::Text(rest));
    }
    Ok(found)
}

/// Fills each `{= name =}` in the template from `values` (spaces inside the braces are
/// optional), applying any filters. A name with no value, an unknown filter, a `{=` never
/// closed, or a hole (a hole has no value here) is an error naming the trouble.
///
/// Example: `fill("{= dir =}/{= stem|pascal =}.ext", &[("dir","src/a"),("stem","b_c")])` is
/// `Ok("src/a/BC.ext")`.
pub fn fill(template: &str, values: &[(&str, &str)]) -> Result<String, String> {
    let shown = excerpt(template);
    pieces(template)?
        .into_iter()
        .map(|piece| match piece {
            Piece::Text(text) => Ok(text.to_string()),
            Piece::Value { name, filters } => filled_value(name, &filters, values, &shown),
            Piece::Hole { name, .. } => {
                Err(format!("a hole has no value here: `{name}` (in `{shown}`)"))
            }
        })
        .collect()
}

/// A value by name with its filters applied left to right.
///
/// Example: `filled_value("c", &["pascal"], &[("c", "cancel_table")], "…")` is
/// `Ok("CancelTable")`.
pub fn filled_value(
    name: &str,
    filters: &[&str],
    values: &[(&str, &str)],
    template: &str,
) -> Result<String, String> {
    let (_, value) = values
        .iter()
        .find(|(known, _)| *known == name)
        .ok_or_else(|| format!("`{name}` is not a name a template may use (in `{template}`)"))?;
    filters
        .iter()
        .try_fold(value.to_string(), |value, filter| {
            apply_filter(filter, &value)
        })
        .map_err(|why| format!("{why} (in `{template}`)"))
}

/// The template as an error quotes it: whole when it is short (a path or a name), else its
/// first line cut short, since a whole file of template says nothing about where the trouble is.
///
/// Example: a six-line template is quoted as its first line and `…`.
fn excerpt(template: &str) -> String {
    const LONGEST: usize = 60;
    let first = template.lines().next().unwrap_or_default();
    let is_short = !template.contains('\n') && template.chars().count() <= LONGEST;
    if is_short {
        return template.to_string();
    }
    let cut: String = first.chars().take(LONGEST).collect();
    format!("{cut}…")
}

fn refuse_double_braces(template: &str, shown: &str) -> Result<(), String> {
    if template.contains("{{") && template.contains("}}") {
        Err(format!(
            "\"{{{{ }}}}\" is not this template language; write {{= name =}} (in `{shown}`)"
        ))
    } else {
        Ok(())
    }
}

/// What sits between `{=` and `=}`: a hole when it starts with `?`, else a value.
fn piece_inside<'t>(inside: &'t str, template: &str) -> Result<Piece<'t>, String> {
    match inside.strip_prefix('?') {
        Some(hole) => hole_piece(hole.trim(), template),
        None => value_piece(inside, template),
    }
}

fn value_piece<'t>(inside: &'t str, template: &str) -> Result<Piece<'t>, String> {
    let mut parts = inside.split('|').map(str::trim);
    let name = parts.next().unwrap_or_default();
    refuse_control_words(name, template)?;
    refuse_empty_name(name, template)?;
    Ok(Piece::Value {
        name,
        filters: parts.collect(),
    })
}

fn hole_piece<'t>(inside: &'t str, template: &str) -> Result<Piece<'t>, String> {
    let (head, instruction) = inside.split_once('|').unwrap_or((inside, ""));
    let (name, kind) = head.split_once(':').ok_or_else(|| {
        format!(
            "the hole `{}` has no `: kind` (in `{template}`)",
            head.trim()
        )
    })?;
    let (name, kind) = (name.trim(), kind.trim());
    refuse_empty_name(name, template)?;
    if kind.is_empty() {
        return Err(format!(
            "the hole `{name}` has no `: kind` (in `{template}`)"
        ));
    }
    Ok(Piece::Hole {
        name,
        kind,
        instruction: instruction.trim(),
    })
}

fn refuse_control_words(name: &str, template: &str) -> Result<(), String> {
    let first_word = name.split_whitespace().next().unwrap_or_default();
    if ["if", "for", "else", "end"].contains(&first_word) {
        Err(format!(
            "`{{= {first_word} =}}` arrives in milestone 2 (in `{template}`)"
        ))
    } else {
        Ok(())
    }
}

fn refuse_empty_name(name: &str, template: &str) -> Result<(), String> {
    if name.is_empty() {
        Err(format!("a `{{= =}}` with no name (in `{template}`)"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_names_with_or_without_spaces() {
        assert_eq!(
            fill("{= dir =}/{=stem=}.x", &[("dir", "a"), ("stem", "b")]),
            Ok("a/b.x".into())
        );
    }

    #[test]
    fn an_unknown_or_unclosed_name_is_an_error() {
        assert!(fill("{= nope =}", &[]).unwrap_err().contains("nope"));
        assert!(
            fill("{= dir", &[("dir", "a")])
                .unwrap_err()
                .contains("never closed")
        );
    }

    #[test]
    fn text_without_braces_is_unchanged() {
        assert_eq!(fill("plain", &[]), Ok("plain".into()));
    }

    #[test]
    fn a_filter_changes_the_value() {
        assert_eq!(
            fill("{= c|pascal =}", &[("c", "cancel_table")]),
            Ok("CancelTable".into())
        );
    }

    #[test]
    fn filters_apply_left_to_right() {
        assert_eq!(
            fill("{= c | pascal | kebab =}", &[("c", "cancel_table")]),
            Ok("cancel-table".into())
        );
        assert_eq!(
            fill("{= c|sentence|upper =}", &[("c", "cancel_table")]),
            Ok("CANCEL_TABLE".into())
        );
    }

    #[test]
    fn an_unknown_filter_names_the_known_ones() {
        let error = fill("{= c|title =}", &[("c", "x")]).unwrap_err();
        assert!(
            error.contains("title") && error.contains("snake"),
            "{error}"
        );
    }

    #[test]
    fn pieces_split_text_values_and_holes() {
        assert_eq!(
            pieces("a {= x|snake =} {= ? h: block | do it =}"),
            Ok(vec![
                Piece::Text("a "),
                Piece::Value {
                    name: "x",
                    filters: vec!["snake"]
                },
                Piece::Text(" "),
                Piece::Hole {
                    name: "h",
                    kind: "block",
                    instruction: "do it"
                },
            ])
        );
    }

    #[test]
    fn a_hole_without_a_kind_is_an_error() {
        assert!(pieces("{= ? h =}").unwrap_err().contains("no `: kind`"));
        assert!(
            pieces("{= ? h: | x =}")
                .unwrap_err()
                .contains("no `: kind`")
        );
    }

    #[test]
    fn a_hole_is_refused_by_fill() {
        let error = fill("{= ? h: block | x =}", &[]).unwrap_err();
        assert!(error.contains("a hole has no value here"), "{error}");
    }

    #[test]
    fn double_braces_and_if_are_refused_with_their_sentences() {
        let braces = pieces("{{ name }}").unwrap_err();
        assert!(
            braces.contains("\"{{ }}\" is not this template language; write {= name =}"),
            "{braces}"
        );
        let condition = pieces("{= if x =}y{= end =}").unwrap_err();
        assert!(condition.contains("arrives in milestone 2"), "{condition}");
        let each = pieces("{= for x in xs =}").unwrap_err();
        assert!(each.contains("arrives in milestone 2"), "{each}");
        assert!(pieces("{=  =}").unwrap_err().contains("no name"));
    }

    #[test]
    fn a_long_template_is_quoted_by_its_first_line_only() {
        let template = format!("first line\n{{= if x =}}\n{}", "more\n".repeat(50));
        let error = pieces(&template).unwrap_err();
        assert!(error.contains("(in `first line…`)"), "{error}");
        assert!(error.len() < 120, "{error}");
    }
}
