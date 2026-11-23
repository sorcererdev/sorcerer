//! Inputs, templates and the plan, on a copy of bistro with the vendored grammars.

mod support;

use sorcerer_core::adapters::files_view::FilesView;
use spellbook::adapters::generator_dir::generators_in;
use spellbook::domain::generator::Generator;
use spellbook::domain::inputs::{Given, InputStop, Values, input_stop_text};
use spellbook::usecases::choices_for::{Resolved, choices_for, resolve_inputs};
use support::*;

fn given(pairs: &[(&str, &str)]) -> Given {
    Given(
        pairs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect(),
    )
}

/// The command generator and a view of the project at a directory.
fn command_in(dir: &std::path::Path) -> (Generator, FilesView) {
    let (project, view) = project_with_vendored_grammars(dir);
    let generator = generators_in(&project).remove(0).unwrap();
    (generator, view)
}

fn ready(resolved: Resolved) -> Values {
    match resolved {
        Resolved::Ready(values) => values,
        Resolved::Stop(stop) => panic!("stopped: {}", input_stop_text(&stop)),
    }
}

fn stopped(resolved: Resolved) -> InputStop {
    match resolved {
        Resolved::Stop(stop) => stop,
        Resolved::Ready(values) => panic!("ready: {values:?}"),
    }
}

#[test]
fn app_is_taken_when_one_folder_has_main_py() {
    let dir = bistro_project();
    let (generator, view) = command_in(dir.path());
    let values = ready(resolve_inputs(&view, &generator, &given(&[("command", "cancel_table")])));
    assert_eq!(values.get("app"), Some("bistro"));
    assert_eq!(values.get("service"), Some("booking"));
}

#[test]
fn services_are_the_folders_holding_models() {
    let dir = bistro_project();
    write_file(dir.path(), "src/bistro/kitchen/models/__init__.py", "");
    let (generator, view) = command_in(dir.path());
    assert_eq!(
        choices_for(&view, &generator, &Given::default(), "service"),
        Ok(vec!["booking".to_string(), "kitchen".to_string()])
    );
}

#[test]
fn a_star_is_one_segment() {
    let dir = bistro_project();
    write_file(dir.path(), "src/bistro/web/x/main.py", "");
    let (generator, view) = command_in(dir.path());
    assert_eq!(
        choices_for(&view, &generator, &Given::default(), "app"),
        Ok(vec!["bistro".to_string()])
    );
}

#[test]
fn a_given_value_not_among_the_choices_lists_them() {
    let dir = bistro_project();
    write_file(dir.path(), "src/bistro/kitchen/models/__init__.py", "");
    let (generator, view) = command_in(dir.path());
    let stop = stopped(resolve_inputs(
        &view,
        &generator,
        &given(&[("service", "bar"), ("command", "x")]),
    ));
    assert_eq!(
        input_stop_text(&stop),
        "service: \"bar\" is not one of: booking, kitchen"
    );
}

#[test]
fn a_missing_input_stops_with_its_prompt_and_choices() {
    let dir = bistro_project();
    write_file(dir.path(), "src/bistro/kitchen/models/__init__.py", "");
    let (generator, view) = command_in(dir.path());
    let stop = stopped(resolve_inputs(&view, &generator, &given(&[("command", "x")])));
    assert_eq!(
        input_stop_text(&stop),
        "service: the service it belongs to (booking, kitchen)"
    );
    assert_eq!(stop.input(), Some("service"));
}

#[test]
fn a_missing_ident_stops_with_its_prompt() {
    let dir = bistro_project();
    let (generator, view) = command_in(dir.path());
    let stop = stopped(resolve_inputs(&view, &generator, &Given::default()));
    assert_eq!(
        input_stop_text(&stop),
        "command: the command, a verb phrase (cancel_booking, close_table)"
    );
}

#[test]
fn an_ident_that_is_not_a_name_is_refused() {
    let dir = bistro_project();
    let (generator, view) = command_in(dir.path());
    let stop = stopped(resolve_inputs(&view, &generator, &given(&[("command", "x/y")])));
    assert!(matches!(stop, InputStop::NotAName { .. }), "{stop:?}");
}

#[test]
fn an_ident_in_any_case_form_is_the_same_words() {
    let dir = bistro_project();
    let (generator, view) = command_in(dir.path());
    for typed in ["cancel_table", "CancelTable", "cancel-table", "cancelTable"] {
        let values = ready(resolve_inputs(&view, &generator, &given(&[("command", typed)])));
        assert_eq!(values.get("command_"), Some("cancel_table"), "{typed}");
        assert_eq!(values.get("Command"), Some("CancelTable"), "{typed}");
    }
}

#[test]
fn derive_sees_inputs_and_earlier_derives() {
    let dir = bistro_project();
    let (generator, view) = command_in(dir.path());
    let values = ready(resolve_inputs(&view, &generator, &given(&[("command", "CancelTable")])));
    assert_eq!(values.get("dir"), Some("src/bistro/booking/commands"));
    assert_eq!(
        values.get("module"),
        Some("bistro.booking.commands.cancel_table")
    );
    assert_eq!(values.inputs().len(), 3);
}

#[test]
fn the_command_template_renders_three_placeholder_holes() {
    use spellbook::domain::holes::{HoleBook, HoleOrigin};
    use spellbook::domain::holes_table::placeholder;
    use spellbook::domain::template::render;

    let dir = bistro_project();
    let (generator, view) = command_in(dir.path());
    let values = ready(resolve_inputs(&view, &generator, &given(&[("command", "cancel_table")])));
    let python = loaded("python");
    let placeholder_of = |kind: &str, name: &str, instruction: &str| {
        placeholder(&python.language, kind, name, instruction).map_err(|e| e.to_string())
    };
    let main = render(&generator.templates["command.py.dt"], &values, &generator.holes, &placeholder_of).unwrap();
    let test = render(&generator.templates["command__test.py.dt"], &values, &generator.holes, &placeholder_of).unwrap();
    let holes = HoleBook::default()
        .with_file("cancel_table.py", &main.holes)
        .unwrap()
        .with_file("cancel_table__test.py", &test.holes)
        .unwrap()
        .holes();
    let placeholders: Vec<(&str, &str, u32)> = holes
        .iter()
        .filter(|hole| hole.source == HoleOrigin::Placeholder)
        .map(|hole| (hole.name.as_str(), hole.places[0].file.as_str(), hole.places[0].line))
        .collect();
    assert_eq!(
        placeholders,
        [
            ("field", "cancel_table.py", 10),
            ("type", "cancel_table.py", 10),
            ("handle", "cancel_table.py", 30)
        ]
    );
    let field = holes.iter().find(|hole| hole.name == "field").unwrap();
    assert_eq!(field.places[1].file, "cancel_table__test.py");
    let lines: Vec<&str> = main.text.lines().collect();
    assert_eq!(lines[0], "\"\"\"Cancel table.\"\"\"");
    assert_eq!(lines[9], "    todo_field: object");
    assert_eq!(
        lines[29],
        "        raise NotImplementedError(\"hole handle: its first case, from the test file beside it\")"
    );
}

/// A generator `tiny` with one ident input and one edit.
fn write_tiny(root: &std::path::Path, create: &str, body: &str, templates: &[(&str, &str)]) {
    let toml = format!(
        "name = \"Tiny\"\nlanguage = \"python\"\n\n[inputs.name]\ntype = \"ident\"\n\n[[edits]]\ncreate = \"{create}\"\n{body}\n"
    );
    write_generator(root, "tiny", &toml, templates);
}

/// Every file under a directory and its bytes, to see that nothing changed.
fn tree_of(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(tree_of(&path));
        } else {
            found.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
    found.sort();
    found
}

#[test]
fn the_command_plan_has_three_files_one_skipped_and_three_holes() {
    use spellbook::domain::plan::FileStatus;

    let dir = bistro_project();
    let rig = PlanRig::new(dir.path());
    let plan = rig.plan("command", &[("command", "cancel_table")]).unwrap();
    let files: Vec<(&str, FileStatus)> = plan.files.iter().map(|f| (f.path.as_str(), f.status)).collect();
    assert_eq!(
        files,
        [
            ("src/bistro/booking/commands/__init__.py", FileStatus::Skipped),
            ("src/bistro/booking/commands/cancel_table.py", FileStatus::New),
            ("src/bistro/booking/commands/cancel_table__test.py", FileStatus::New),
        ]
    );
    let holes: Vec<&str> = plan.placeholder_holes().iter().map(|h| h.name.as_str()).collect();
    assert_eq!(holes, ["field", "type", "handle"]);
    assert_eq!(plan.id.len(), 8);
    assert_eq!(plan.files[1].lines, 30);
}

#[test]
fn the_plan_writes_nothing() {
    let dir = bistro_project();
    let before = tree_of(dir.path());
    let rig = PlanRig::new(dir.path());
    rig.plan("command", &[("command", "cancel_table")]).unwrap();
    assert_eq!(tree_of(dir.path()), before);
}

#[test]
fn a_path_out_of_the_project_fails_the_plan() {
    use spellbook::domain::plan::PlanStop;

    for create in ["../x.py", "/abs/x.py", "C:\\\\x.py", ".sorcerer/x.py", ".git/x", "src/.env.local", "src/../../x.py"] {
        let dir = bistro_project();
        write_tiny(dir.path(), create, "text = \"\"", &[]);
        let rig = PlanRig::new(dir.path());
        let stop = rig.plan("tiny", &[("name", "x")]).unwrap_err();
        assert!(matches!(stop, PlanStop::PathRefused { .. }), "{create}: {stop:?}");
    }
}

#[test]
fn an_existing_file_fails_the_plan_by_default() {
    use spellbook::domain::plan::{PlanStop, plan_stop_text};

    let dir = bistro_project();
    write_file(dir.path(), "src/bistro/booking/commands/cancel_table.py", "x = 1\n");
    let rig = PlanRig::new(dir.path());
    let stop = rig.plan("command", &[("command", "cancel_table")]).unwrap_err();
    assert_eq!(
        stop,
        PlanStop::Exists { path: "src/bistro/booking/commands/cancel_table.py".into() }
    );
    assert!(plan_stop_text(&stop).contains("is there already"));
}

#[test]
fn a_template_whose_placeholder_does_not_parse_fails_with_template_and_line() {
    use spellbook::domain::plan::{PlanStop, plan_stop_text};

    let dir = bistro_project();
    write_tiny(
        dir.path(),
        "src/{= name =}.py",
        "template = \"tiny.py.dt\"",
        &[("tiny.py.dt", "import os\n\nx = ({= ? h: expr | the value =})\n")],
    );
    let rig = PlanRig::new(dir.path());
    let stop = rig.plan("tiny", &[("name", "x")]).unwrap_err();
    let PlanStop::DoesNotParse { template, file, line } = &stop else {
        panic!("{stop:?}");
    };
    assert_eq!((template.as_str(), file.as_str()), ("templates/tiny.py.dt", "src/x.py"));
    assert!(*line >= 3, "{line}");
    assert!(plan_stop_text(&stop).contains("would not parse"));
}

#[test]
fn a_file_no_language_covers_is_not_parsed() {
    use spellbook::domain::plan::FileStatus;

    let dir = bistro_project();
    write_tiny(dir.path(), "src/{= name =}.html", "text = \"<div\"", &[]);
    let rig = PlanRig::new(dir.path());
    let plan = rig.plan("tiny", &[("name", "x")]).unwrap();
    assert_eq!(plan.files[0].status, FileStatus::New);
    assert_eq!(plan.files[0].text, "<div");
}

#[test]
fn a_placeholder_hole_in_an_uncovered_file_is_refused() {
    use spellbook::domain::plan::{PlanStop, plan_stop_text};

    let dir = bistro_project();
    write_tiny(
        dir.path(),
        "src/{= name =}.html",
        "template = \"page.html.dt\"",
        &[("page.html.dt", "<p>{= ? body: block | the page =}</p>\n")],
    );
    let rig = PlanRig::new(dir.path());
    let stop = rig.plan("tiny", &[("name", "x")]).unwrap_err();
    assert_eq!(
        plan_stop_text(&stop),
        "no language covers src/x.html, so its hole body has no placeholder"
    );
    assert!(matches!(stop, PlanStop::NoLanguageForHole { .. }));
}
