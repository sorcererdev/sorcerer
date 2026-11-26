//! The rules files of the corpora: their path patterns on the bases' real paths, and the loader.

mod support;

use sorcerer_core::domain::project::Project;
use spellbook::adapters::rules_toml::load_rules;
use spellbook::domain::rule::{RuleBody, parse_rules};
use spellbook::domain::rule_query::query_text;
use support::*;

fn corpus_rules(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/spellbook")
        .join(name)
        .join("spellbook/rules.toml");
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn path_patterns_over_every_files_line_in_both_corpora() {
    let python = parse_rules(&corpus_rules("bistro")).unwrap();
    let by_id = |id: &str| python.iter().find(|rule| rule.id == id).unwrap();
    let commands = by_id("commands-use-ports");
    let applies = |rule: &spellbook::domain::rule::Rule, path: &str| rule.applies_to(path);
    assert!(applies(
        commands,
        "src/bistro/booking/commands/create_booking.py"
    ));
    assert!(applies(
        commands,
        "src/bistro/booking/queries/get_day_schedule.py"
    ));
    assert!(!applies(
        commands,
        "src/bistro/booking/commands/__init__.py"
    ));
    assert!(!applies(
        commands,
        "src/bistro/booking/commands/create_booking__test.py"
    ));
    assert!(!applies(
        commands,
        "src/bistro/booking/ports/id_generator/id_generator.py"
    ));
    let slots = commands
        .files
        .matches("src/bistro/booking/commands/create_booking.py")
        .unwrap();
    assert_eq!(slots["kind"], "commands");
    assert_eq!(slots["service"], "booking");
    let web = by_id("web-uses-commands");
    assert!(applies(web, "src/bistro/web/bookings/routes.py"));
    assert!(applies(web, "src/bistro/web/app.py"));
    assert!(!applies(web, "src/bistro/web/bookings/routes__test.py"));
    let adapters = by_id("adapters-in-main");
    assert!(applies(
        adapters,
        "src/bistro/booking/commands/create_booking.py"
    ));
    assert!(!applies(adapters, "src/bistro/main.py"));
    assert!(!applies(
        adapters,
        "src/bistro/booking/ports/booking_repository/sqlite_booking_repository.py"
    ));
    assert!(!applies(adapters, "src/bistro/web/app.py"));
    let failures = by_id("failures-returned");
    assert!(applies(
        failures,
        "src/bistro/booking/commands/cancel_booking.py"
    ));
    assert!(!applies(
        failures,
        "src/bistro/booking/queries/get_day_schedule.py"
    ));

    let rust = parse_rules(&corpus_rules("bistro-rs")).unwrap();
    assert_eq!(rust.len(), 1);
    let panics = &rust[0];
    assert!(applies(panics, "src/booking/commands/create_booking.rs"));
    assert!(!applies(panics, "src/booking/commands/mod.rs"));
    assert!(!applies(panics, "src/booking/queries/get_day_schedule.rs"));
}

#[test]
fn the_corpus_rules_compile_to_queries_the_grammars_accept() {
    for (name, language) in [("bistro", "python"), ("bistro-rs", "rust")] {
        let loaded = loaded(language);
        for rule in parse_rules(&corpus_rules(name)).unwrap() {
            let RuleBody::Match(form) = &rule.body else {
                continue;
            };
            let text = query_text(form);
            sorcerer_core::domain::query::compile(&loaded.grammar, &rule.id, &text)
                .unwrap_or_else(|e| panic!("{}: {e}\n{text}", rule.id));
        }
    }
}

#[test]
fn no_rules_file_means_no_rules_and_a_bad_one_names_itself() {
    let dir = bistro_project();
    let project = Project::open(dir.path()).unwrap();
    assert!(load_rules(&project).unwrap().is_empty());
    write_file(
        dir.path(),
        ".sorcerer/spellbook/rules.toml",
        "[rules.a]\nfiles = \"x\"\n",
    );
    let problem = load_rules(&Project::open(dir.path()).unwrap()).unwrap_err();
    assert_eq!(
        problem.to_string(),
        ".sorcerer/spellbook/rules.toml: rule a: `message` is missing: the sentence a finding says"
    );
}

#[test]
fn a_finding_carries_its_span_and_the_runner_finds_the_node_around_it() {
    use spellbook::adapters::query_rules::QueryRules;
    use spellbook::ports::rule_runner::RuleRunner;
    use std::path::Path;

    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    let wired = wired_at(dir.path());
    let rules = load_rules(&wired.project).unwrap();
    let commands: Vec<&spellbook::domain::rule::Rule> = rules
        .iter()
        .filter(|rule| rule.id == "commands-use-ports")
        .collect();
    let runner = QueryRules::new(&wired.grammars);
    let file = Path::new("src/bistro/booking/commands/close_table.py");
    let text = "\"\"\"Close a table.\"\"\"\n\nimport sqlite3\n\n\ndef handle(path):\n    conn = sqlite3.connect(path)\n    return conn\n";
    let found = runner.findings_in(file, text, &commands).unwrap();
    let import = found.iter().find(|f| f.line == 3).unwrap();
    assert_eq!(
        &text[import.span.start_byte..import.span.end_byte],
        "import sqlite3"
    );
    assert_eq!((import.span.start_row, import.span.end_row), (2, 2));
    let kinds = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
    let itself = runner
        .enclosing(
            file,
            text,
            import.span,
            &kinds(&["import_statement", "import_from_statement"]),
        )
        .unwrap()
        .unwrap();
    assert_eq!(itself, import.span);

    let call = found.iter().find(|f| f.line == 7).unwrap();
    assert_eq!(call.node_text, "sqlite3.connect");
    let statement = runner
        .enclosing(
            file,
            text,
            call.span,
            &kinds(&["expression_statement", "return_statement"]),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        &text[statement.start_byte..statement.end_byte],
        "conn = sqlite3.connect(path)"
    );
    assert_eq!(
        runner
            .enclosing(file, text, call.span, &kinds(&["class_definition"]))
            .unwrap(),
        None
    );
}
