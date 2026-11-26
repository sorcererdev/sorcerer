//! A fix planned and applied on a copy of bistro: `use-port` on `close_table.py`.

mod support;

use spellbook::adapters::query_rules::QueryRules;
use spellbook::adapters::rules_toml::load_rules;
use spellbook::domain::inputs::Given;
use spellbook::domain::plan::{Applied, FileStatus, PlanStop, plan_stop_text};
use spellbook::usecases::apply_plan::{ChangesLanded, apply_plan};
use spellbook::usecases::fixes_for::fixes_for;
use spellbook::usecases::plan_fix::{FindingAt, FixDeps, plan_fix};
use support::*;

const MODULE: &str = CLOSE_TABLE_PATH;
const TEST: &str = CLOSE_TABLE_TEST_PATH;

fn port(name: &str) -> Given {
    Given(
        [("port".to_string(), name.to_string())]
            .into_iter()
            .collect(),
    )
}

fn at(line: u32) -> FindingAt {
    FindingAt {
        file: MODULE.into(),
        line,
        rule: None,
        index: 1,
    }
}

#[test]
fn the_finding_offers_use_port_asking_only_the_port() {
    let dir = bistro_with_the_fix();
    let wired = wired_at(dir.path());
    let rules = load_rules(&wired.project).unwrap();
    let rule = rules.iter().find(|r| r.id == "commands-use-ports").unwrap();
    let offers = fixes_for(
        &wired.view,
        &wired.generators,
        rule,
        MODULE,
        "import sqlite3",
    );
    assert_eq!(offers.len(), 2);
    assert_eq!(offers[1].label, "Add a port and use it");
    assert_eq!(offers[1].generator, "add-port");
    assert_eq!(offers[1].missing, vec!["port".to_string()]);
    assert_eq!(offers[0].label, "Use a port the service has");
    assert_eq!(offers[0].generator, "use-port");
    assert_eq!(offers[0].inputs["unit"], "close_table");
    assert_eq!(offers[0].inputs["kind"], "commands");
    assert_eq!(offers[0].missing, vec!["port".to_string()]);
}

#[test]
fn use_port_on_the_import_changes_two_files_and_a_second_run_has_nothing_to_do() {
    let dir = bistro_with_the_fix();
    let wired = wired_at(dir.path());
    let rules = load_rules(&wired.project).unwrap();
    let runner = QueryRules::new(&wired.grammars);
    let deps = FixDeps {
        plan: wired.plan_deps(),
        runner: &runner,
        rules: &rules,
        generators: &wired.generators,
    };
    let plan = plan_fix(&deps, &at(4), &port("table_repository"))
        .unwrap_or_else(|stop| panic!("{}", plan_stop_text(&stop)));
    let statuses: Vec<(&str, FileStatus)> = plan
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.status))
        .collect();
    assert_eq!(
        statuses,
        vec![(MODULE, FileStatus::Changed), (TEST, FileStatus::Changed)]
    );
    let module = &plan.files[0];
    assert!(!module.text.contains("import sqlite3\n"), "{}", module.text);
    assert!(module.text.contains(
        "from dataclasses import dataclass\n\nfrom bistro.shared.result import Ok\nfrom bistro.booking.ports.table_repository.table_repository import TableRepository\n"
    ), "{}", module.text);
    assert!(module.text.contains(
        "    def __init__(self, table_repository: TableRepository) -> None:\n        \"\"\"Takes the ports it uses; main.py passes their adapters.\"\"\"\n        self.table_repository = table_repository\n"
    ), "{}", module.text);
    let test = &plan.files[1];
    assert!(
        test.text
            .contains("    return CloseTable(table_repository=InMemoryTableRepository())\n"),
        "{}",
        test.text
    );
    let fix = plan.fix.as_ref().unwrap();
    assert_eq!((fix.rule.as_str(), fix.index), ("commands-use-ports", 1));

    let applied = apply_plan(&wired.apply_deps(), plan, ChangesLanded::NotYet).unwrap();
    assert_eq!(applied.refused, None);
    assert_eq!(applied.changed, vec![MODULE.to_string(), TEST.to_string()]);

    let wired = wired_at(dir.path());
    let deps = FixDeps {
        plan: wired.plan_deps(),
        runner: &runner,
        rules: &rules,
        generators: &wired.generators,
    };
    // The connect line moved down by one: the import line went, the port's came, __init__ grew.
    let text = std::fs::read_to_string(dir.path().join(MODULE)).unwrap();
    let connect_line = text
        .lines()
        .position(|l| l.contains("sqlite3.connect"))
        .unwrap() as u32
        + 1;
    let call = plan_fix(&deps, &at(connect_line), &port("table_repository"))
        .unwrap_or_else(|stop| panic!("{}", plan_stop_text(&stop)));
    assert_eq!(call.files.len(), 1);
    assert!(call.files[0].text.contains(
        "        raise NotImplementedError(\"hole port_call: the same as conn = sqlite3.connect(bistro.db) through self.table_repository\")\n"
    ), "{}", call.files[0].text);
    assert_eq!(call.placeholder_holes().len(), 1);

    std::fs::write(
        dir.path().join(MODULE),
        text.replace("        conn = sqlite3.connect(\"bistro.db\")\n", ""),
    )
    .unwrap();
    let wired = wired_at(dir.path());
    let deps = FixDeps {
        plan: wired.plan_deps(),
        runner: &runner,
        rules: &rules,
        generators: &wired.generators,
    };
    assert_eq!(
        plan_fix(&deps, &at(4), &port("table_repository")).unwrap_err(),
        PlanStop::NothingToDo(Applied::Fix)
    );
}
