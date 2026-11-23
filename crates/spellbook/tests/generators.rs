//! Generators and the language files' `[holes]`, on the vendored grammars.

mod support;

use sorcerer_core::domain::facts::parse_errors_of;
use sorcerer_core::domain::language::Language;
use spellbook::domain::holes_table::{holes_table, placeholder};
use support::*;

/// A host snippet per kind: where a placeholder of that kind sits in real code.
fn host(language: &str, kind: &str, hole: &str) -> String {
    match (language, kind) {
        ("python", "ident") => format!("class A:\n    {hole}: int\n"),
        ("python", "type") => format!("class A:\n    x: {hole}\n"),
        ("python", "block") => format!("def f():\n    {hole}\n"),
        ("python", "expr") => format!("x = {hole}\n"),
        ("rust", "ident") => format!("struct A {{\n    {hole}: u32,\n}}\n"),
        ("rust", "type") => format!("struct A {{\n    x: {hole},\n}}\n"),
        ("rust", "block") => format!("fn f() {{\n    {hole}\n}}\n"),
        ("rust", "expr") => format!("fn f() {{\n    let x = {hole};\n}}\n"),
        _ => unreachable!(),
    }
}

#[test]
fn each_shipped_kind_fills_and_parses() {
    for language in ["python", "rust"] {
        let loaded = loaded(language);
        let table = holes_table(&loaded.language).unwrap();
        let kinds: Vec<&str> = table.kinds.keys().map(String::as_str).collect();
        assert_eq!(kinds, ["block", "expr", "ident", "type"], "{language}");
        for kind in kinds {
            let hole = placeholder(&loaded.language, kind, "handle", "its first case").unwrap();
            let code = host(language, kind, &hole);
            assert_eq!(
                parse_errors_of(&loaded, &code).unwrap(),
                Vec::<u32>::new(),
                "{language} {kind}:\n{code}"
            );
        }
    }
}

#[test]
fn a_block_placeholder_says_its_name_and_instruction() {
    let loaded = loaded("python");
    let hole = placeholder(&loaded.language, "block", "handle", "its first case").unwrap();
    assert_eq!(
        hole,
        "raise NotImplementedError(\"hole handle: its first case\")"
    );
}

#[test]
fn a_missing_kind_names_the_language_and_its_kinds() {
    let loaded = loaded("python");
    let error = placeholder(&loaded.language, "statement", "h", "x")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("the language python has no hole kind statement (it has: block, expr, ident, type)"),
        "{error}"
    );
}

#[test]
fn a_language_without_holes_has_no_kinds() {
    let mut language: Language = loaded("python").language.clone();
    language.rest.remove("holes");
    assert!(holes_table(&language).unwrap().kinds.is_empty());
    let error = placeholder(&language, "block", "h", "x").unwrap_err().to_string();
    assert!(error.contains("no [holes] table"), "{error}");
}
