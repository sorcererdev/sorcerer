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
