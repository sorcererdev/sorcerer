#![allow(clippy::too_many_lines)]
mod support;

use sorcerer_core::domain::facts::{Facts, facts_of, parse_errors_of};
use sorcerer_core::domain::unit::units_of;

fn facts(lang: &str, text: &str, unit: &str) -> Facts {
    let l = support::languages_with_vendored_grammars()
        .load(lang, &support::test_cache())
        .unwrap();
    let us = units_of(&l, text).unwrap();
    let u = us
        .iter()
        .find(|u| u.name == unit)
        .unwrap_or_else(|| panic!("no {unit} in {us:?}"));
    facts_of(&l, text, u).unwrap()
}

#[test]
fn a_clean_function_has_no_facts() {
    let f = facts("python", "def f(x):\n    return x + 1\n", "f");
    assert_eq!(
        f,
        Facts {
            parse_errors: vec![],
            todo_lines: vec![],
            body_empty: false
        }
    );
}

#[test]
fn a_parse_error_inside_the_unit_is_its_line() {
    let text = "def f(x):\n    return x +\n\ndef g():\n    return 1\n";
    let f = facts("python", text, "f");
    assert_eq!(f.parse_errors, vec![2]);
    assert!(facts("python", text, "g").parse_errors.is_empty());
}

#[test]
fn todo_and_fixme_in_comments_are_todo_lines() {
    let text = "def f():\n    # TODO: handle None\n    x = \"TODO\"\n    # FIXME later\n    # todo lower case\n    return x\n";
    assert_eq!(facts("python", text, "f").todo_lines, vec![2, 4]);
    let rs = "fn f() {\n    // TODO a\n    /* ok\n       FIXME b */\n    1\n}\n";
    assert_eq!(facts("rust", rs, "f").todo_lines, vec![2, 4]);
}

#[test]
fn a_body_of_only_pass_or_ellipsis_or_todo_macro_is_empty() {
    assert!(facts("python", "def f():\n    pass\n", "f").body_empty);
    assert!(facts("python", "def f():\n    # later\n    ...\n", "f").body_empty);
    assert!(facts("rust", "fn f() {\n    todo!()\n}\n", "f").body_empty);
    assert!(facts("rust", "fn f() {\n    unimplemented!();\n}\n", "f").body_empty);
    assert!(facts("rust", "fn f() {}\n", "f").body_empty);
}

#[test]
fn a_body_with_a_statement_is_not() {
    assert!(!facts("python", "def f():\n    return 1\n", "f").body_empty);
    assert!(!facts("rust", "fn f() {\n    println!(\"x\")\n}\n", "f").body_empty);
}

#[test]
fn parse_errors_of_lists_every_error_line_in_the_file() {
    let l = support::languages_with_vendored_grammars()
        .load("python", &support::test_cache())
        .unwrap();
    let broken =
        "def f(x):\n    return x +\n\ndef g():\n    return 1\n\ndef h(y):\n    return y *\n";
    assert_eq!(parse_errors_of(&l, broken).unwrap(), vec![2, 8]);
    let clean = "def f(x):\n    return x + 1\n";
    assert_eq!(parse_errors_of(&l, clean).unwrap(), Vec::<u32>::new());
}
