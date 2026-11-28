//! Generators and the language files' `[holes]`, on the vendored grammars.

mod support;

use sorcerer_core::domain::facts::parse_errors_of;
use sorcerer_core::domain::language::Language;
use spellbook::adapters::generator_dir::{generator_named, generators_in, load_generator};
use spellbook::domain::generator::{Edit, EditBody, HoleSource, InputKind, OnExists};
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
        // a statement among others: what a fix's `replace` text puts where a statement was
        ("rust", "stmt") => format!("fn f() {{\n    let x = 1;\n    {hole}\n    g(x);\n}}\n"),
        _ => unreachable!(),
    }
}

#[test]
fn each_shipped_kind_fills_and_parses() {
    for (language, shipped) in [
        ("python", &["block", "expr", "ident", "type"][..]),
        ("rust", &["block", "expr", "ident", "stmt", "type"][..]),
    ] {
        let loaded = loaded(language);
        let table = holes_table(&loaded.language).unwrap();
        let kinds: Vec<&str> = table.kinds.keys().map(String::as_str).collect();
        assert_eq!(kinds, shipped, "{language}");
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
    let rust = support::loaded("rust");
    let hole = placeholder(
        &rust.language,
        "stmt",
        "port_call",
        "the same through the port",
    )
    .unwrap();
    assert_eq!(
        hole,
        "todo!(\"hole port_call: the same through the port\");"
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
    assert_eq!(generator.edits[0].body(), &EditBody::Text(String::new()));
    let on_exists = |edit: &Edit| match edit {
        Edit::Create { on_exists, .. } => Some(*on_exists),
        Edit::Insert { .. } => None,
    };
    assert_eq!(on_exists(&generator.edits[0]), Some(OnExists::Skip));
    assert_eq!(on_exists(&generator.edits[1]), Some(OnExists::Error));
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

/// The problem loading `listing` once its file and template are changed.
fn listing_problem(toml: &str, template: &str) -> String {
    let dir = bistro_with_listing();
    write_generator(dir.path(), "listing", toml, &[("listing.py.dt", template)]);
    let project = vendored_project(dir.path());
    generator_named(&project, "listing")
        .unwrap()
        .unwrap_err()
        .to_string()
}

fn listing_toml_problem(from: &str, to: &str) -> String {
    assert!(LISTING_TOML.contains(from), "{from}");
    listing_problem(&LISTING_TOML.replacen(from, to, 1), LISTING_PY)
}

fn listing_template_problem(template: &str) -> String {
    listing_problem(LISTING_TOML, template)
}

#[test]
fn a_list_input_loads() {
    let dir = bistro_with_listing();
    let project = vendored_project(dir.path());
    let generator = generator_named(&project, "listing").unwrap().unwrap();
    let InputKind::List { pattern, keys_of } = &generator.inputs[1].1.kind else {
        panic!("{:?}", generator.inputs[1]);
    };
    assert_eq!(pattern.written(), "{name}:{kind}:{target?}");
    assert_eq!(keys_of["kind"], "kinds");
    let no_item = listing_toml_problem("item    = \"{name}:{kind}:{target?}\"\n", "");
    assert!(
        no_item.contains(
            "[inputs.fields]: a list needs `item`, the pattern of one item (\"{name}:{kind}\")"
        ),
        "{no_item}"
    );
    let colour = listing_toml_problem("{ kind = \"kinds\" }", "{ colour = \"kinds\" }");
    assert!(
        colour.contains(
            "[inputs.fields]: keys_of names `colour`, which is not a part of `{name}:{kind}:{target?}`"
        ),
        "{colour}"
    );
    let sorts = listing_toml_problem("{ kind = \"kinds\" }", "{ kind = \"sorts\" }");
    assert!(
        sorts.contains(
            "[inputs.fields]: keys_of names table `sorts`, which tables.toml does not have"
        ),
        "{sorts}"
    );
}

#[test]
fn tables_toml_mistakes_say_where() {
    let dir = bistro_with_listing();
    write_file(dir.path(), TABLES_PATH, "[kinds.text]\npy = 1\n");
    let project = vendored_project(dir.path());
    let number = generator_named(&project, "listing")
        .unwrap()
        .unwrap_err()
        .to_string();
    assert!(
        number.contains("tables.toml [kinds.text] py: a value is text"),
        "{number}"
    );
    write_file(
        dir.path(),
        TABLES_PATH,
        &format!("{TABLES_TOML}\n[fields.a]\npy = \"x\"\n"),
    );
    let named = generator_named(&project, "listing")
        .unwrap()
        .unwrap_err()
        .to_string();
    assert!(
        named.contains(
            "tables.toml: table `fields` has the name of an input of generator `listing`; rename one"
        ),
        "{named}"
    );
}

#[test]
fn a_for_over_a_text_input_is_refused() {
    let why = listing_template_problem("{= for f in model =}{= end =}\n");
    assert!(why.contains("(templates/listing.py.dt)"), "{why}");
    assert!(
        why.contains("`{= for f in model =}` (line 1): `model` is not a list input"),
        "{why}"
    );
}

#[test]
fn an_unknown_part_is_refused() {
    let why = listing_template_problem("x = 1\n{= for f in fields =}{= f.colour =}{= end =}\n");
    assert!(
        why.contains(
            "`f.colour` (line 2): an item of `fields` has the parts name, kind, target, optional"
        ),
        "{why}"
    );
}

#[test]
fn a_missing_column_is_refused() {
    let dir = bistro_with_listing();
    write_file(
        dir.path(),
        TABLES_PATH,
        &format!("{TABLES_TOML}\n[kinds.text]\nsql = \"TEXT\"\n")
            .replace("[kinds.text]\npy = \"str\"\n\n", ""),
    );
    let template = "x = 1\n{= for f in fields =}{= kinds[f.kind].sql =}{= end =}\n";
    write_generator(
        dir.path(),
        "listing",
        LISTING_TOML,
        &[("listing.py.dt", template)],
    );
    let project = vendored_project(dir.path());
    let why = generator_named(&project, "listing")
        .unwrap()
        .unwrap_err()
        .to_string();
    assert!(
        why.contains("`kinds[f.kind].sql` (line 2): row `int` of table `kinds` has no `sql`"),
        "{why}"
    );
}

#[test]
fn a_lookup_key_must_be_a_keys_of_part() {
    let why =
        listing_template_problem("x = 1\n{= for f in fields =}{= kinds[f.name].py =}{= end =}\n");
    assert!(
        why.contains(
            "`kinds[f.name]` (line 2): `name` is not checked against `kinds`; look up a part keys_of names"
        ),
        "{why}"
    );
}

#[test]
fn a_hole_inside_for_is_refused() {
    let why = listing_template_problem(
        "{= for f in fields =}\nx = 1\n{= ? check: block | do it =}\n{= end =}\n",
    );
    assert!(
        why.contains(
            "a hole inside `{= for =}` would repeat; move `{= ? check … =}` (line 3) outside the loop"
        ),
        "{why}"
    );
}

#[test]
fn a_loop_name_may_not_hide_an_input() {
    let why = listing_template_problem("{= for model in fields =}{= end =}\n");
    assert!(
        why.contains(
            "`{= for model in fields =}` (line 1): `model` is an input; name the item something else"
        ),
        "{why}"
    );
}

#[test]
fn control_tags_in_a_path_are_refused() {
    let why = listing_toml_problem(
        "create   = \"src/bistro/{= model|snake =}.py\"",
        "create   = \"src/{= if model =}a.py\"",
    );
    assert!(
        why.contains("`{= if =}` works only in a template body (in `src/{= if model =}a.py`)"),
        "{why}"
    );
}

/// The command generator with one more edit after its first.
fn with_insert(edit: &str) -> String {
    toml_problem(
        "on_exists = \"skip\"",
        &format!("on_exists = \"skip\"\n\n[[edits]]\n{edit}"),
    )
}

#[test]
fn an_insert_edit_says_what_it_lacks_or_mixes() {
    let both = toml_problem(
        "on_exists = \"skip\"",
        "on_exists = \"skip\"\ninsert = \"x\"",
    );
    assert!(both.contains("give create or insert, not both"), "{both}");
    let no_at = with_insert("insert = \"x\"\ntext = \"y\"");
    assert!(
        no_at.contains("at is missing: where the lines go"),
        "{no_at}"
    );
    let on_exists =
        with_insert("insert = \"x\"\nat = \"end of imports\"\ntext = \"y\"\non_exists = \"skip\"");
    assert!(
        on_exists.contains("`on_exists` does not go with insert"),
        "{on_exists}"
    );
    let at_in_create = with_insert("create = \"x\"\nat = \"end of imports\"\ntext = \"y\"");
    assert!(
        at_in_create.contains("`at` does not go with create"),
        "{at_in_create}"
    );
}

#[test]
fn an_insert_phrase_and_its_unless_are_checked_at_load() {
    let phrase = with_insert("insert = \"x\"\nat = \"end of cases in update\"\ntext = \"y\"");
    assert!(
        phrase.contains("[[edits]] 2 at: `end of cases in update`: unknown target"),
        "{phrase}"
    );
    let unless = with_insert(
        "insert = \"x\"\nat = \"end of imports\"\ntext = \"y\"\nunless = { matches = \"({= command =}\" }",
    );
    assert!(
        unless.contains("[[edits]] 2 unless: the regex `({= command =}` does not compile"),
        "{unless}"
    );
    let name = with_insert("insert = \"x\"\nat = \"end of imports\"\ntext = \"{= nope =}\"");
    assert!(
        name.contains("[[edits]] 2 text: `nope` is not an input"),
        "{name}"
    );
}

#[test]
fn an_if_without_its_end_names_its_line() {
    let why = template_problem("from dataclasses", "{= if app =}from dataclasses");
    assert!(why.contains("(templates/command.py.dt)"), "{why}");
    assert!(
        why.contains("the `{= if =}` on line 3 has no `{= end =}`"),
        "{why}"
    );
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
fn a_query_exclude_naming_a_later_input_is_refused() {
    let why = toml_problem(
        "glob  = \"src/{= app =}/*/models\"",
        "glob  = \"src/{= app =}/*/models\"\nexclude = [\"src/{= command =}/x\"]",
    );
    assert!(
        why.contains("[query.services] exclude: a query naming an input not yet given (command"),
        "{why}"
    );
}

#[test]
fn a_query_leaves_out_the_paths_its_exclude_matches() {
    use spellbook::domain::generator::QueryValue;
    use spellbook::domain::inputs::query_choices;
    let files: Vec<std::path::PathBuf> = ["src/ports/a.rs", "src/ports/mod.rs", "src/ports/b.rs"]
        .iter()
        .map(Into::into)
        .collect();
    let found = |exclude: &[&str]| {
        let exclude: Vec<String> = exclude.iter().map(ToString::to_string).collect();
        query_choices(&files, "src/ports/*.rs", &exclude, QueryValue::Stem).unwrap()
    };
    assert_eq!(found(&[]), ["a", "b", "mod"]);
    assert_eq!(found(&["src/ports/mod.rs"]), ["a", "b"]);
    assert_eq!(found(&["src/ports/mod.rs", "**/b.rs"]), ["a"]);
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
fn an_instruction_with_a_brace_is_refused() {
    let why = template_problem("| its first case, from", "| the map {id: table}, from");
    assert!(
        why.contains("the hole handle's instruction must be plain text"),
        "{why}"
    );
}

#[test]
fn a_generator_id_that_climbs_out_is_not_found() {
    let dir = bistro_project();
    write_file(
        dir.path(),
        ".sorcerer/spellbook/x/generator.toml",
        "x = 1\n",
    );
    let project = vendored_project(dir.path());
    for id in ["../x", "..", "a/b", "", "a\\b"] {
        assert!(generator_named(&project, id).is_none(), "{id:?}");
    }
}

#[test]
fn template_and_text_together_are_refused() {
    let why = toml_problem(
        "template = \"command.py.dt\"",
        "template = \"command.py.dt\"\ntext = \"x\"",
    );
    assert!(
        why.contains("[[edits]] 2: give one of template, text or copy"),
        "{why}"
    );
}

#[test]
fn a_default_outside_the_choices_is_refused() {
    let why = toml_problem(
        "one_of = \"apps\"",
        "choices = [\"front\", \"back\"]\ndefault = \"side\"",
    );
    assert!(
        why.contains("default \"side\" is not one of the choices (front, back)"),
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

#[test]
fn a_copy_edit_says_where_its_file_belongs_and_what_it_mixes() {
    let outside = with_insert("create = \"x.js\"\ncopy = \"templates/x.js\"");
    assert!(
        outside.contains("copy = \"templates/x.js\": write a path under files/"),
        "{outside}"
    );
    let missing = with_insert("create = \"x.js\"\ncopy = \"files/x.js\"");
    assert!(
        missing.contains("copy \"files/x.js\" is not a file in files/"),
        "{missing}"
    );
    let mixed = with_insert("create = \"x.js\"\ncopy = \"files/x.js\"\ntext = \"y\"");
    assert!(
        mixed.contains("give one of template, text or copy"),
        "{mixed}"
    );
    let in_insert = with_insert("insert = \"x\"\nat = \"end of imports\"\ncopy = \"files/x.js\"");
    assert!(
        in_insert.contains("`copy` does not go with insert"),
        "{in_insert}"
    );
}

#[test]
fn a_copy_edits_file_is_read_as_bytes() {
    let dir = bistro_project();
    let toml = COMMAND_TOML.replacen(
        "on_exists = \"skip\"",
        "on_exists = \"skip\"\n\n[[edits]]\ncreate = \"static/mark.woff2\"\ncopy = \"files/fonts/mark.woff2\"",
        1,
    );
    write_generator(
        dir.path(),
        "command",
        &toml,
        &[
            ("command.py.dt", COMMAND_PY),
            ("command__test.py.dt", COMMAND_TEST_PY),
        ],
    );
    let font = [0x77, 0x4f, 0x46, 0x32, 0x00, 0xff, 0xfe];
    write_bytes(
        dir.path(),
        ".sorcerer/spellbook/generators/command/files/fonts/mark.woff2",
        &font,
    );
    let project = vendored_project(dir.path());
    let generator = generator_named(&project, "command").unwrap().unwrap();
    assert_eq!(generator.files["fonts/mark.woff2"], font.to_vec());
    assert_eq!(
        generator.edits[1].body(),
        &EditBody::Copy("fonts/mark.woff2".into())
    );
}

#[test]
fn notes_are_templates_over_the_generators_names() {
    let unknown = toml_problem(
        "[[edits]]",
        "[notes]\nafter = [\"run {= nope =}\"]\n\n[[edits]]",
    );
    assert!(
        unknown.contains("[notes] after: `nope` is not an input or a derived name"),
        "{unknown}"
    );
    let not_a_list = toml_problem("[[edits]]", "[notes]\nafter = \"x\"\n\n[[edits]]");
    assert!(
        not_a_list.contains("[notes]: after must be a list of text"),
        "{not_a_list}"
    );
}

#[test]
fn double_braces_are_text_unless_they_hold_a_generators_name() {
    let braced = template_problem("class ", "# {{ command }}\nclass ");
    assert!(
        braced.contains("`{{ command }}` is written as it is; to put the value of command here write {= command =}"),
        "{braced}"
    );
    let dir = bistro_project();
    write_generator(
        dir.path(),
        "command",
        COMMAND_TOML,
        &[
            (
                "command.py.dt",
                &COMMAND_PY.replacen("class ", "# {{ title }} {% if x %}\nclass ", 1),
            ),
            ("command__test.py.dt", COMMAND_TEST_PY),
        ],
    );
    let project = vendored_project(dir.path());
    assert!(generator_named(&project, "command").unwrap().is_ok());
}
