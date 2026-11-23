//! Checking files against the corpora's rules, on a copy of bistro and a copy of bistro-rs.

mod support;

use std::path::PathBuf;

use spellbook::domain::finding::Finding;
use spellbook::domain::rule::Severity;
use spellbook::usecases::check_files::check_files;
use spellbook::usecases::check_project::check_project;
use support::*;

const COMMAND: &str = "src/bistro/booking/commands/close_table.py";

const SQLITE_COMMAND: &str = "\"\"\"Close a table.\"\"\"\n\nfrom dataclasses import dataclass\nimport sqlite3\n\n\n@dataclass(frozen=True)\nclass CloseTableInput:\n    table: str\n";

fn bistro_with(command: &str) -> tempfile::TempDir {
    let dir = bistro_project();
    use_corpus_rules(dir.path(), "bistro");
    write_file(dir.path(), COMMAND, command);
    dir
}

fn check(dir: &std::path::Path, files: &[&str]) -> Result<Vec<Finding>, String> {
    let wired = wired_at(dir);
    let checking = wired.checking()?;
    let files: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
    check_files(&checking.deps(), &files).map_err(|e| e.to_string())
}

fn lines(found: &[Finding]) -> Vec<(&str, u32)> {
    found.iter().map(|f| (f.rule.as_str(), f.line)).collect()
}

#[test]
fn sqlite3_in_a_command_is_a_finding() {
    let dir = bistro_with(SQLITE_COMMAND);
    let found = check(dir.path(), &[COMMAND]).unwrap();
    assert_eq!(lines(&found), [("commands-use-ports", 4)]);
    assert_eq!(found[0].file, COMMAND);
    assert_eq!(found[0].severity, Severity::Warn);
    assert!(
        found[0]
            .message
            .starts_with("Commands and queries talk to ports")
    );
}

#[test]
fn an_allow_with_a_reason_silences_it() {
    let text = SQLITE_COMMAND.replace(
        "import sqlite3",
        "# spellbook:allow commands-use-ports — migration, issue 12\nimport sqlite3",
    );
    let dir = bistro_with(&text);
    assert_eq!(check(dir.path(), &[COMMAND]).unwrap(), []);
    // on the finding's own line too
    let text = SQLITE_COMMAND.replace(
        "import sqlite3",
        "import sqlite3  # spellbook:allow commands-use-ports — migration",
    );
    let dir = bistro_with(&text);
    assert_eq!(check(dir.path(), &[COMMAND]).unwrap(), []);
}

#[test]
fn an_allow_without_a_reason_leaves_a_warning_on_the_comment() {
    let text = SQLITE_COMMAND.replace(
        "import sqlite3",
        "# spellbook:allow commands-use-ports\nimport sqlite3",
    );
    let dir = bistro_with(&text);
    let found = check(dir.path(), &[COMMAND]).unwrap();
    assert_eq!(lines(&found), [("allow-without-reason", 4)]);
    assert_eq!(found[0].severity, Severity::Warn);
    assert!(
        found[0].message.contains("gives no reason"),
        "{}",
        found[0].message
    );
}

#[test]
fn an_allow_naming_no_rule_is_unknown_rule() {
    let text = SQLITE_COMMAND.replace(
        "import sqlite3",
        "# spellbook:allow commands-use-port — typo\nimport sqlite3",
    );
    let dir = bistro_with(&text);
    let found = check(dir.path(), &[COMMAND]).unwrap();
    // the typo silences nothing: the finding stands too
    assert_eq!(
        lines(&found),
        [("unknown-rule", 4), ("commands-use-ports", 5)]
    );
    assert!(
        found[0].message.contains("the rules are: "),
        "{}",
        found[0].message
    );
}

#[test]
fn excluded_files_are_not_checked() {
    let dir = bistro_with(SQLITE_COMMAND);
    write_file(
        dir.path(),
        ".sorcerer/config.toml",
        "[spellbook]\nexclude = [\"src/bistro/booking/commands/close_*\"]\n",
    );
    assert_eq!(check(dir.path(), &[COMMAND]).unwrap(), []);
}

#[test]
fn one_finding_per_line_per_rule() {
    // `import sqlite3, os` matches the import rule once; `sqlite3.connect` attribute twice on a
    // line is still one finding for the line.
    let text = "import os\nconn = (sqlite3.connect(\"a\"), sqlite3.connect(\"b\"))\n";
    let dir = bistro_with(text);
    let found = check(dir.path(), &[COMMAND]).unwrap();
    assert_eq!(lines(&found), [("commands-use-ports", 2)]);
}

#[test]
fn the_fingerprint_ignores_whitespace() {
    let dir = bistro_with("import   sqlite3\n");
    let first = check(dir.path(), &[COMMAND]).unwrap();
    let dir = bistro_with("\n\nimport sqlite3\n");
    let second = check(dir.path(), &[COMMAND]).unwrap();
    assert_eq!(first[0].fingerprint, second[0].fingerprint);
    assert_ne!(first[0].line, second[0].line);
}

#[test]
fn a_query_without_finding_is_refused() {
    let dir = bistro_with(SQLITE_COMMAND);
    write_file(
        dir.path(),
        ".sorcerer/spellbook/rules.toml",
        "[rules.no-capture]\nfiles = \"src/**.py\"\nmessage = \"m\"\nquery = \"(import_statement) @import\"\n",
    );
    let error = check(dir.path(), &[COMMAND]).unwrap_err();
    assert_eq!(
        error,
        "query rule no-capture: its query has no `@finding` capture; capture the node to report as `@finding`"
    );
}

#[test]
fn a_query_rule_finds_what_it_captures() {
    let dir = bistro_with(SQLITE_COMMAND);
    write_file(
        dir.path(),
        ".sorcerer/spellbook/rules.toml",
        "[rules.imports]\nfiles = \"src/**.py\"\nmessage = \"an import\"\nseverity = \"hint\"\nquery = \"(import_statement name: (dotted_name) @finding)\"\n",
    );
    let found = check(dir.path(), &[COMMAND]).unwrap();
    assert_eq!(lines(&found), [("imports", 4)]);
    assert_eq!(found[0].severity, Severity::Hint);
}

#[test]
fn report_json_is_versioned_and_kept_per_file() {
    let dir = bistro_with(SQLITE_COMMAND);
    check(dir.path(), &[COMMAND]).unwrap();
    let path = dir.path().join(".sorcerer/state/spellbook/report.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["version"], 1);
    assert_eq!(json["data"]["checked"], "2026-11-22T12:00:00Z");
    assert_eq!(json["data"]["findings"][0]["rule"], "commands-use-ports");
    assert_eq!(json["data"]["findings"][0]["file"], COMMAND);
    // fixing the file and checking it again clears its findings and keeps nothing else
    write_file(dir.path(), COMMAND, "import os\n");
    check(dir.path(), &[COMMAND]).unwrap();
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(json["data"]["findings"], serde_json::json!([]));
}

#[test]
fn checking_the_project_rewrites_the_report_whole() {
    let dir = bistro_with(SQLITE_COMMAND);
    let wired = wired_at(dir.path());
    let checking = wired.checking().unwrap();
    let found = check_project(&checking.deps()).unwrap();
    assert_eq!(found.iter().filter(|f| f.file == COMMAND).count(), 1);
    // the base's own adapters and tests do not break the rules
    assert!(found.iter().all(|f| f.file == COMMAND), "{found:#?}");
}

#[test]
fn a_panic_in_a_rust_command_is_a_finding() {
    let dir = tempfile::tempdir().unwrap();
    copy_tree(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/scry/bistro-rs/base"),
        dir.path(),
    );
    std::fs::create_dir_all(dir.path().join(".sorcerer")).unwrap();
    use_corpus_rules(dir.path(), "bistro-rs");
    let file = "src/booking/commands/create_booking.rs";
    assert_eq!(check(dir.path(), &[file]).unwrap(), []);
    let text = std::fs::read_to_string(dir.path().join(file)).unwrap();
    let broken = text.replacen(
        "        let Some(table) = self.bookings.table(&input.table_id) else {",
        "        if input.party == 0 {\n            panic!(\"empty party\");\n        }\n        let Some(table) = self.bookings.table(&input.table_id) else {",
        1,
    );
    assert_ne!(text, broken);
    std::fs::write(dir.path().join(file), broken).unwrap();
    let found = check(dir.path(), &[file]).unwrap();
    assert_eq!(lines(&found), [("commands-no-panic", 47)]);
}
