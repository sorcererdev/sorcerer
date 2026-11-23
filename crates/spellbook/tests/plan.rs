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
