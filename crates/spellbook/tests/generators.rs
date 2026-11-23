//! Generators and the language files' `[holes]`, on the vendored grammars.

mod support;

use sorcerer_core::domain::facts::parse_errors_of;
use sorcerer_core::domain::language::Language;
use spellbook::adapters::generator_dir::{generators_in, load_generator};
use spellbook::domain::generator::{EditBody, HoleSource, InputKind, OnExists};
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
        error.contains(
            "the language python has no hole kind statement (it has: block, expr, ident, type)"
        ),
        "{error}"
    );
}

#[test]
fn a_language_without_holes_has_no_kinds() {
    let mut language: Language = loaded("python").language.clone();
    language.rest.remove("holes");
    assert!(holes_table(&language).unwrap().kinds.is_empty());
    let error = placeholder(&language, "block", "h", "x")
        .unwrap_err()
        .to_string();
    assert!(error.contains("no [holes] table"), "{error}");
}

/// The command generator's problem, after one change to its toml or its main template.
fn problem_after(toml: &str, template: &str) -> String {
    let dir = bistro_project();
    write_generator(
        dir.path(),
        "command",
        toml,
        &[
            ("command.py.dt", template),
            ("command__test.py.dt", COMMAND_TEST_PY),
        ],
    );
    let project = vendored_project(dir.path());
    let generator_dir = dir.path().join(".sorcerer/spellbook/generators/command");
    load_generator(&generator_dir, &project.languages)
        .unwrap_err()
        .to_string()
}

fn toml_problem(from: &str, to: &str) -> String {
    assert!(COMMAND_TOML.contains(from), "{from}");
    problem_after(&COMMAND_TOML.replacen(from, to, 1), COMMAND_PY)
}

fn template_problem(from: &str, to: &str) -> String {
    assert!(COMMAND_PY.contains(from), "{from}");
    problem_after(COMMAND_TOML, &COMMAND_PY.replacen(from, to, 1))
}

#[test]
fn the_bistro_command_generator_loads() {
    let dir = bistro_project();
    let project = vendored_project(dir.path());
    let loaded = generators_in(&project);
    assert_eq!(loaded.len(), 1);
    let generator = loaded[0].as_ref().unwrap();
    assert_eq!(generator.id, "command");
    assert_eq!(generator.name, "Command");
    assert_eq!(generator.language, "python");
    let inputs: Vec<&str> = generator.inputs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(inputs, ["app", "service", "command"]);
    assert_eq!(
        generator.inputs[1].1.kind,
        InputKind::Enum {
            one_of: "services".into()
        }
    );
    assert_eq!(generator.edits.len(), 3);
    assert_eq!(generator.edits[0].body, EditBody::Text(String::new()));
    assert_eq!(generator.edits[0].on_exists, OnExists::Skip);
    assert_eq!(generator.edits[1].on_exists, OnExists::Error);
    assert_eq!(
        generator.holes["purpose"],
        HoleSource::Derived {
            value: "{= command|sentence =}.".into()
        }
    );
}

#[test]
fn inputs_and_derive_keep_the_files_order() {
    let toml = COMMAND_TOML
        .replace("[inputs.app]", "[inputs.zeta]")
        .replace("{= app =}", "{= zeta =}");
    let dir = bistro_project();
    write_generator(
        dir.path(),
        "command",
        &toml,
        &[
            (
                "command.py.dt",
                &COMMAND_PY.replace("{= app =}", "{= zeta =}"),
            ),
            (
                "command__test.py.dt",
                &COMMAND_TEST_PY.replace("{= app =}", "{= zeta =}"),
            ),
        ],
    );
    let project = vendored_project(dir.path());
    let generator = generators_in(&project).remove(0).unwrap();
    let inputs: Vec<&str> = generator.inputs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(inputs, ["zeta", "service", "command"]);
    let derive: Vec<&str> = generator.derive.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(derive, ["Command", "command_", "dir", "module"]);
}

#[test]
fn an_unknown_key_suggests_text_for_content() {
    let why = toml_problem("text      = \"\"", "content   = \"\"");
    assert!(
        why.contains(
            "generator command (generator.toml): [[edits]] 1: `content` is not a key; write `text`"
        ),
        "{why}"
    );
}

#[test]
fn a_list_input_arrives_in_milestone_2() {
    let why = toml_problem("type   = \"ident\"", "type   = \"list\"");
    assert!(
        why.contains("[inputs.command]: type = \"list\" arrives in milestone 2"),
        "{why}"
    );
}

#[test]
fn an_insert_edit_arrives_in_milestone_2() {
    let why = toml_problem(
        "on_exists = \"skip\"",
        "on_exists = \"skip\"\ninsert = \"x\"",
    );
    assert!(why.contains("`insert` arrives in milestone 2"), "{why}");
}

#[test]
fn an_if_in_a_template_arrives_in_milestone_2() {
    let why = template_problem("from dataclasses", "{= if app =}from dataclasses");
    assert!(why.contains("(templates/command.py.dt)"), "{why}");
    assert!(why.contains("arrives in milestone 2"), "{why}");
}

#[test]
fn a_function_style_name_says_the_filter() {
    let why = template_problem("class {= Command =}:", "class {= pascalCase(command) =}:");
    assert!(why.contains("write {= command|pascal =}"), "{why}");
}

#[test]
fn a_query_naming_a_later_input_is_refused() {
    let why = toml_problem("src/{= app =}/*/models", "src/{= command =}/*/models");
    assert!(
        why.contains(
            "[query.services] glob: a query naming an input not yet given (command, before service)"
        ),
        "{why}"
    );
}

#[test]
fn a_hole_kind_the_language_lacks_is_refused() {
    let why = template_problem("{= ? handle: block |", "{= ? handle: statement |");
    assert!(
        why.contains(
            "the language python has no hole kind statement (it has: block, expr, ident, type)"
        ),
        "{why}"
    );
}

#[test]
fn one_hole_with_two_kinds_is_refused() {
    let why = template_problem(
        "{= ? handle: block | its first case, from the test file beside it =}",
        "{= ? field: block | its first input =}",
    );
    assert!(
        why.contains("the hole field is written with another kind or instruction"),
        "{why}"
    );
}

#[test]
fn an_instruction_with_a_quote_is_refused() {
    let why = template_problem("| its first case, from", "| its \"first\" case, from");
    assert!(
        why.contains("the hole handle's instruction must be plain text"),
        "{why}"
    );
}

#[test]
fn template_and_text_together_are_refused() {
    let why = toml_problem(
        "template = \"command.py.dt\"",
        "template = \"command.py.dt\"\ntext = \"x\"",
    );
    assert!(
        why.contains("[[edits]] 2: give template or text, not both"),
        "{why}"
    );
}

#[test]
fn a_missing_template_file_is_named() {
    let why = toml_problem("template = \"command.py.dt\"", "template = \"nope.py.dt\"");
    assert!(
        why.contains("template \"nope.py.dt\" is not a file in templates/"),
        "{why}"
    );
}

#[test]
fn a_language_the_project_lacks_is_refused() {
    let why = toml_problem("language    = \"python\"", "language    = \"cobol\"");
    assert!(
        why.contains("language = \"cobol\" is not a language of this project"),
        "{why}"
    );
}

#[test]
fn a_broken_generator_does_not_hide_the_others() {
    let dir = bistro_project();
    write_generator(
        dir.path(),
        "broken",
        "name = \"Broken\"\nlanguag = \"python\"\n",
        &[],
    );
    let project = vendored_project(dir.path());
    let loaded = generators_in(&project);
    assert_eq!(loaded.len(), 2);
    let problem = loaded[0].as_ref().unwrap_err();
    assert_eq!(problem.generator, "broken");
    assert!(
        problem.why.contains("did you mean `language`?"),
        "{problem}"
    );
    assert_eq!(loaded[1].as_ref().unwrap().id, "command");
}
