//! Name forms: one name written in any case style, split into its words, and written again in
//! the style a template asks for (`{= command|pascal =}`).

/// The filters a template may apply to a value, in the order the error message lists them.
pub const FILTERS: &[&str] = &["snake", "pascal", "camel", "kebab", "upper", "sentence", "words"];

/// The words of a name, lowercase, whatever case style it was written in. Underscores, dashes
/// and spaces separate words, and so does a capital letter after a small one. Digits stay with
/// the word they follow.
///
/// Example: `cancel_table`, `CancelTable`, `cancel-table`, `cancelTable` and `Cancel table` all
/// give `["cancel", "table"]`; `HTTPServer2` gives `["http", "server2"]`.
pub fn words_of(name: &str) -> Vec<String> {
    name.split(['_', '-', ' '])
        .flat_map(case_words)
        .map(|word| word.to_lowercase())
        .collect()
}

/// The value written as the filter says: `snake` gives `cancel_table`, `pascal` `CancelTable`,
/// `camel` `cancelTable`, `kebab` `cancel-table`, `upper` `CANCEL_TABLE`, `sentence`
/// `Cancel table` and `words` `cancel table`. A filter not in [`FILTERS`] is an error that lists
/// the known ones.
pub fn apply_filter(filter: &str, value: &str) -> Result<String, String> {
    let words = words_of(value);
    match filter {
        "snake" => Ok(words.join("_")),
        "pascal" => Ok(words.iter().map(|word| capitalised(word)).collect()),
        "camel" => Ok(camel(&words)),
        "kebab" => Ok(words.join("-")),
        "upper" => Ok(words.join("_").to_uppercase()),
        "sentence" => Ok(capitalised(&words.join(" "))),
        "words" => Ok(words.join(" ")),
        _ => Err(unknown_filter_text(filter)),
    }
}

/// Splits one separator-free piece at its case changes: before a capital that follows a small
/// letter or a digit, and before the last capital of a run when a small letter follows it
/// (`HTTPServer` splits as `HTTP`, `Server`).
fn case_words(piece: &str) -> Vec<String> {
    let chars: Vec<char> = piece.chars().collect();
    let starts_word = |i: usize| {
        let (before, here) = (chars[i - 1], chars[i]);
        let next_is_small = chars.get(i + 1).is_some_and(|c| c.is_lowercase());
        here.is_uppercase()
            && (before.is_lowercase()
                || before.is_ascii_digit()
                || (before.is_uppercase() && next_is_small))
    };
    let mut words: Vec<String> = Vec::new();
    for (i, c) in chars.iter().enumerate() {
        match words.last_mut() {
            Some(word) if !starts_word(i) => word.push(*c),
            _ => words.push(c.to_string()),
        }
    }
    words
}

fn camel(words: &[String]) -> String {
    match words.split_first() {
        Some((first, rest)) => {
            first.clone() + &rest.iter().map(|word| capitalised(word)).collect::<String>()
        }
        None => String::new(),
    }
}

fn capitalised(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn unknown_filter_text(filter: &str) -> String {
    format!(
        "`{filter}` is not a filter (the filters are: {})",
        FILTERS.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_case_form_gives_the_same_words() {
        for name in [
            "cancel_table",
            "CancelTable",
            "cancel-table",
            "cancelTable",
            "Cancel table",
        ] {
            assert_eq!(words_of(name), ["cancel", "table"], "{name}");
        }
        assert_eq!(words_of("HTTPServer2"), ["http", "server2"]);
        assert_eq!(words_of("table2_name"), ["table2", "name"]);
    }

    #[test]
    fn each_filter_on_cancel_table() {
        let cases = [
            ("snake", "cancel_table"),
            ("pascal", "CancelTable"),
            ("camel", "cancelTable"),
            ("kebab", "cancel-table"),
            ("upper", "CANCEL_TABLE"),
            ("sentence", "Cancel table"),
            ("words", "cancel table"),
        ];
        for (filter, expected) in cases {
            assert_eq!(apply_filter(filter, "cancel_table"), Ok(expected.into()));
        }
    }

    #[test]
    fn an_unknown_filter_lists_the_known_ones() {
        let error = apply_filter("title", "x").unwrap_err();
        assert!(error.contains("title") && error.contains("snake, pascal"), "{error}");
    }
}
