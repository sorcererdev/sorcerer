//! Path templates in language files: `{= name =}` marks a place a value goes.

/// Fills each `{= name =}` in the template from `values` (spaces inside the braces are
/// optional). A name with no value, or a `{=` never closed, is an error naming the trouble.
///
/// Example: `fill("{= dir =}/{= stem =}_test.ext", &[("dir","src/a"),("stem","b")])` is
/// `Ok("src/a/b_test.ext")`.
pub fn fill(template: &str, values: &[(&str, &str)]) -> Result<String, String> {
    let mut filled = String::new();
    let mut rest = template;
    while let Some(open) = rest.find("{=") {
        filled.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let close = after
            .find("=}")
            .ok_or_else(|| format!("`{{=` is never closed in `{template}`"))?;
        let name = after[..close].trim();
        let (_, value) = values.iter().find(|(n, _)| *n == name).ok_or_else(|| {
            format!("`{name}` is not a name a template may use (in `{template}`)")
        })?;
        filled.push_str(value);
        rest = &after[close + 2..];
    }
    filled.push_str(rest);
    Ok(filled)
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
}
