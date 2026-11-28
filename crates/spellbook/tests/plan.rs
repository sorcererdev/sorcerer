//! Inputs, templates and the plan, on a copy of bistro with the vendored grammars.

mod support;

use sorcerer_core::adapters::files_view::FilesView;
use sorcerer_core::domain::template_blocks::Datum;
use spellbook::adapters::generator_dir::{generator_named, generators_in};
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
    let values = ready(resolve_inputs(
        &view,
        &generator,
        &given(&[("command", "cancel_table")]),
    ));
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
    let stop = stopped(resolve_inputs(
        &view,
        &generator,
        &given(&[("command", "x")]),
    ));
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
    let stop = stopped(resolve_inputs(
        &view,
        &generator,
        &given(&[("command", "x/y")]),
    ));
    assert!(matches!(stop, InputStop::NotAName { .. }), "{stop:?}");
}

#[test]
fn an_ident_in_any_case_form_is_the_same_words() {
    let dir = bistro_project();
    let (generator, view) = command_in(dir.path());
    for typed in ["cancel_table", "CancelTable", "cancel-table", "cancelTable"] {
        let values = ready(resolve_inputs(
            &view,
            &generator,
            &given(&[("command", typed)]),
        ));
        assert_eq!(values.get("command_"), Some("cancel_table"), "{typed}");
        assert_eq!(values.get("Command"), Some("CancelTable"), "{typed}");
    }
}

#[test]
fn derive_sees_inputs_and_earlier_derives() {
    let dir = bistro_project();
    let (generator, view) = command_in(dir.path());
    let values = ready(resolve_inputs(
        &view,
        &generator,
        &given(&[("command", "CancelTable")]),
    ));
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
    let values = ready(resolve_inputs(
        &view,
        &generator,
        &given(&[("command", "cancel_table")]),
    ));
    let python = loaded("python");
    let placeholder_of = |kind: &str, name: &str, instruction: &str| {
        placeholder(&python.language, kind, name, instruction).map_err(|e| e.to_string())
    };
    let main = render(
        &generator.templates["command.py.dt"],
        &values,
        &generator.holes,
        &placeholder_of,
    )
    .unwrap();
    let test = render(
        &generator.templates["command__test.py.dt"],
        &values,
        &generator.holes,
        &placeholder_of,
    )
    .unwrap();
    let holes = HoleBook::default()
        .with_file("cancel_table.py", &main.holes)
        .unwrap()
        .with_file("cancel_table__test.py", &test.holes)
        .unwrap()
        .holes();
    let placeholders: Vec<(&str, &str, u32)> = holes
        .iter()
        .filter(|hole| hole.source == HoleOrigin::Placeholder)
        .map(|hole| {
            (
                hole.name.as_str(),
                hole.places[0].file.as_str(),
                hole.places[0].line,
            )
        })
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
    let files: Vec<(&str, FileStatus)> = plan
        .files
        .iter()
        .map(|f| (f.path.as_str(), f.status))
        .collect();
    assert_eq!(
        files,
        [
            (
                "src/bistro/booking/commands/__init__.py",
                FileStatus::Skipped
            ),
            (
                "src/bistro/booking/commands/cancel_table.py",
                FileStatus::New
            ),
            (
                "src/bistro/booking/commands/cancel_table__test.py",
                FileStatus::New
            ),
        ]
    );
    let holes: Vec<&str> = plan
        .placeholder_holes()
        .iter()
        .map(|h| h.name.as_str())
        .collect();
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

    for create in [
        "../x.py",
        "/abs/x.py",
        "C:\\\\x.py",
        ".sorcerer/x.py",
        ".git/x",
        "src/.env.local",
        "src/../../x.py",
    ] {
        let dir = bistro_project();
        write_tiny(dir.path(), create, "text = \"\"", &[]);
        let rig = PlanRig::new(dir.path());
        let stop = rig.plan("tiny", &[("name", "x")]).unwrap_err();
        assert!(
            matches!(stop, PlanStop::PathRefused { .. }),
            "{create}: {stop:?}"
        );
    }
}

#[test]
fn an_existing_file_fails_the_plan_by_default() {
    use spellbook::domain::plan::{PlanStop, plan_stop_text};

    let dir = bistro_project();
    write_file(
        dir.path(),
        "src/bistro/booking/commands/cancel_table.py",
        "x = 1\n",
    );
    let rig = PlanRig::new(dir.path());
    let stop = rig
        .plan("command", &[("command", "cancel_table")])
        .unwrap_err();
    assert_eq!(
        stop,
        PlanStop::Exists {
            path: "src/bistro/booking/commands/cancel_table.py".into()
        }
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
        &[(
            "tiny.py.dt",
            "import os\n\nx = ({= ? h: expr | the value =})\n",
        )],
    );
    let rig = PlanRig::new(dir.path());
    let stop = rig.plan("tiny", &[("name", "x")]).unwrap_err();
    let PlanStop::DoesNotParse {
        template,
        file,
        line,
    } = &stop
    else {
        panic!("{stop:?}");
    };
    assert_eq!(
        (template.as_str(), file.as_str()),
        ("templates/tiny.py.dt", "src/x.py")
    );
    assert!(*line >= 3, "{line}");
    assert!(plan_stop_text(&stop).contains("would not parse"));
}

#[test]
fn a_file_no_language_covers_is_not_parsed() {
    use spellbook::domain::plan::FileStatus;

    let dir = bistro_project();
    write_tiny(dir.path(), "src/{= name =}.txt", "text = \"<div\"", &[]);
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
        "src/{= name =}.txt",
        "template = \"page.html.dt\"",
        &[("page.html.dt", "<p>{= ? body: block | the page =}</p>\n")],
    );
    let rig = PlanRig::new(dir.path());
    let stop = rig.plan("tiny", &[("name", "x")]).unwrap_err();
    assert_eq!(
        plan_stop_text(&stop),
        "no language covers src/x.txt, so its hole body has no placeholder"
    );
    assert!(matches!(stop, PlanStop::NoLanguageForHole { .. }));
}

const KINDED_TOML: &str = r#"
name = "Kinded"
language = "python"

[inputs.kind]
type    = "enum"
choices = ["command", "query"]
default = "command"
prompt  = "command or query"

[inputs.thing]
type   = "ident"
prompt = "the thing"

[[edits]]
create = "src/{= thing|snake =}.py"
text   = ""
"#;

fn kinded_in(dir: &std::path::Path) -> (Generator, FilesView) {
    write_generator(dir, "kinded", KINDED_TOML, &[]);
    let (project, view) = project_with_vendored_grammars(dir);
    let generator = spellbook::adapters::generator_dir::generator_named(&project, "kinded")
        .unwrap()
        .unwrap();
    (generator, view)
}

#[test]
fn fixed_choices_are_the_choices_with_no_project_files_needed() {
    let dir = bistro_project();
    let (generator, view) = kinded_in(dir.path());
    // `kind` comes before `thing`, but even asked of a later input's state it is just the list
    assert_eq!(
        choices_for(&view, &generator, &Given::default(), "kind"),
        Ok(vec!["command".to_string(), "query".to_string()])
    );
}

#[test]
fn a_fixed_choice_given_is_checked_and_the_default_stands_in() {
    let dir = bistro_project();
    let (generator, view) = kinded_in(dir.path());
    let values = ready(resolve_inputs(&view, &generator, &given(&[("thing", "x")])));
    assert_eq!(values.get("kind"), Some("command"));
    let values = ready(resolve_inputs(
        &view,
        &generator,
        &given(&[("thing", "x"), ("kind", "query")]),
    ));
    assert_eq!(values.get("kind"), Some("query"));
    match stopped(resolve_inputs(
        &view,
        &generator,
        &given(&[("thing", "x"), ("kind", "service")]),
    )) {
        InputStop::NotAChoice { choices, .. } => assert_eq!(choices, ["command", "query"]),
        other => panic!("{other:?}"),
    }
}

const WIRE_TOML: &str = r#"
name     = "Wire"
summary  = "A command takes one more port"
language = "python"

[inputs.unit]
type   = "ident"
prompt = "The command"

[inputs.port]
type   = "ident"
prompt = "The port"

[derive]
Unit   = "{= unit|pascal =}"
Port   = "{= port|pascal =}"
module = "src/bistro/booking/commands/{= unit|snake =}.py"

[[edits]]
insert = "{= module =}"
at     = "end of imports"
text   = "from bistro.booking.ports.{= port =}.{= port =} import {= Port =}"
unless = { matches = "import .*\\b{= Port =}\\b" }

[[edits]]
insert = "{= module =}"
at     = "end of parameters in {= Unit =}.__init__"
text   = "{= port =}: {= Port =}"
unless = { matches = "\\b{= port =}: {= Port =}\\b" }

[[edits]]
insert = "{= module =}"
at     = "end of function {= Unit =}.__init__"
text   = "self.{= port =} = {= port =}"
unless = { matches = "self\\.{= port =} = {= port =}" }

[[edits]]
insert     = "src/bistro/booking/commands/{= unit|snake =}__nope.py"
at         = "end of imports"
text       = "x = 1"
on_missing = "skip"
"#;

#[test]
fn inserts_change_a_file_in_one_composed_change_and_a_second_plan_has_nothing_to_do() {
    use spellbook::domain::plan::{Applied, FileStatus, PlanStop};

    let dir = bistro_project();
    write_generator(dir.path(), "wire", WIRE_TOML, &[]);
    let rig = PlanRig::new(dir.path());
    let plan = rig
        .plan("wire", &[("unit", "cancel_booking"), ("port", "clock")])
        .unwrap();
    assert_eq!(plan.files.len(), 1, "{:?}", plan.files);
    let file = &plan.files[0];
    assert_eq!(file.path, "src/bistro/booking/commands/cancel_booking.py");
    assert_eq!(file.status, FileStatus::Changed);
    assert_eq!((file.added, file.removed), (3, 1));
    assert!(
        file.text.contains(
            "import BookingRepository\nfrom bistro.booking.ports.clock.clock import Clock\n"
        ),
        "{}",
        file.text
    );
    assert!(
        file.text.contains(
            "    def __init__(self, bookings: BookingRepository, clock: Clock) -> None:\n"
        ),
        "{}",
        file.text
    );
    assert!(
        file.text
            .contains("        self.bookings = bookings\n        self.clock = clock\n"),
        "{}",
        file.text
    );
    let before = std::fs::read_to_string(dir.path().join(&file.path)).unwrap();
    assert_eq!(
        spellbook::domain::change::apply_changes(&before, &file.changes).unwrap(),
        file.text
    );

    std::fs::write(dir.path().join(&file.path), &file.text).unwrap();
    let again = PlanRig::new(dir.path())
        .plan("wire", &[("unit", "cancel_booking"), ("port", "clock")])
        .unwrap_err();
    assert_eq!(again, PlanStop::NothingToDo(Applied::Generation));
}

#[test]
fn an_insert_whose_phrase_misses_stops_the_plan_naming_it() {
    use spellbook::domain::plan::plan_stop_text;

    let dir = bistro_project();
    write_generator(dir.path(), "wire", WIRE_TOML, &[]);
    let stop = PlanRig::new(dir.path())
        .plan("wire", &[("unit", "nope"), ("port", "clock")])
        .unwrap_err();
    assert_eq!(
        plan_stop_text(&stop),
        "[[edits]] 1 insert (src/bistro/booking/commands/nope.py): the file is not there; nothing was written"
    );
    write_file(
        dir.path(),
        "src/bistro/booking/commands/nope.py",
        "import os\n\n\nclass Other:\n    pass\n",
    );
    let stop = PlanRig::new(dir.path())
        .plan("wire", &[("unit", "nope"), ("port", "clock")])
        .unwrap_err();
    assert_eq!(
        plan_stop_text(&stop),
        "[[edits]] 2 insert (src/bistro/booking/commands/nope.py): `end of parameters in Nope.__init__`: no function `Nope.__init__` in nope.py; nothing was written"
    );
}

// ---- list inputs, loops and branches ----

/// The `listing` generator and a view of bistro with it.
fn listing_in(dir: &std::path::Path) -> (Generator, FilesView) {
    let (project, view) = project_with_vendored_grammars(dir);
    let generator = generator_named(&project, "listing").unwrap().unwrap();
    (generator, view)
}

fn listing_values(fields: &str) -> Resolved {
    let dir = bistro_with_listing();
    let (generator, view) = listing_in(dir.path());
    resolve_inputs(
        &view,
        &generator,
        &given(&[("model", "dish"), ("fields", fields)]),
    )
}

fn items_of(values: &Values, name: &str) -> Vec<Datum> {
    match values.bindings().0.get(name) {
        Some(Datum::List(items)) => items.clone(),
        other => panic!("{name}: {other:?}"),
    }
}

fn part(item: &Datum, name: &str) -> Datum {
    match item {
        Datum::Record(parts) => parts[name].clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_list_input_is_parsed_into_items() {
    let values = ready(listing_values("name:text,dish:ref:dishes,notes:text?"));
    let items = items_of(&values, "fields");
    assert_eq!(items.len(), 3);
    assert_eq!(part(&items[1], "target"), Datum::Text("dishes".into()));
    assert_eq!(part(&items[2], "optional"), Datum::Flag(true));
    assert_eq!(part(&items[0], "target"), Datum::Text(String::new()));
    assert_eq!(part(&items[0], "optional"), Datum::Flag(false));
    // the text is kept as typed, for one-line templates and `[[uses]] with`
    assert_eq!(
        values.get("fields"),
        Some("name:text,dish:ref:dishes,notes:text?")
    );
}

#[test]
fn a_bad_item_names_the_part_and_the_keys() {
    let text = |fields: &str| input_stop_text(&stopped(listing_values(fields)));
    let stop = stopped(listing_values("weight:flaot"));
    assert_eq!(stop.input(), Some("fields"));
    assert_eq!(
        input_stop_text(&stop),
        "`fields`: `weight:flaot`: `flaot` is not a kind (one of: int, ref, text)"
    );
    assert_eq!(
        text("a:text,,b:int"),
        "`fields`: an empty item (two commas together)"
    );
    assert_eq!(
        text("a"),
        "`fields`: `a`: `a` has 1 part; an item is `{name}:{kind}:{target?}`"
    );
    assert!(
        text("a:text:x:y").contains("`a:text:x:y` has 4 parts; an item is"),
        "{}",
        text("a:text:x:y")
    );
    assert_eq!(
        text("Bad Name:text"),
        "`fields`: `Bad Name:text`: `Bad Name` is not a name"
    );
}

#[test]
fn an_optional_list_may_be_empty() {
    let dir = bistro_with_listing();
    let toml = LISTING_TOML.replace(
        "prompt  = \"The fields",
        "optional = true\nprompt  = \"The fields",
    );
    write_generator(
        dir.path(),
        "listing",
        &toml,
        &[("listing.py.dt", LISTING_PY)],
    );
    let (generator, view) = listing_in(dir.path());
    let values = ready(resolve_inputs(
        &view,
        &generator,
        &given(&[("model", "dish")]),
    ));
    assert!(items_of(&values, "fields").is_empty());
    let rig = PlanRig::new(dir.path());
    let plan = rig.plan("listing", &[("model", "dish")]).unwrap();
    assert_eq!(plan.files[0].text, "class Dish:\n");
}

#[test]
fn a_list_renders_one_line_per_item() {
    let dir = bistro_with_listing();
    let rig = PlanRig::new(dir.path());
    let plan = rig
        .plan(
            "listing",
            &[
                ("model", "dish"),
                ("fields", "name:text,price:int,notes:text?"),
            ],
        )
        .unwrap();
    assert_eq!(
        plan.files[0].text,
        "class Dish:\n    name: str\n    price: int\n    notes: str | None\n"
    );
}

#[test]
fn a_list_passes_through_uses() {
    let dir = bistro_with_listing();
    let outer = r#"name     = "Outer"
language = "python"

[inputs.model]
type = "ident"

[inputs.fields]
type = "list"
item = "{name}:{kind}"

[[uses]]
generator = "listing"
with      = { model = "{= model =}", fields = "{= fields =}" }
"#;
    write_generator(dir.path(), "outer", outer, &[]);
    let rig = PlanRig::new(dir.path());
    let plan = rig
        .plan(
            "outer",
            &[
                ("model", "dish"),
                ("fields", "name:text,price:int,notes:text?"),
            ],
        )
        .unwrap();
    let file = plan
        .files
        .iter()
        .find(|file| file.path == "src/bistro/dish.py")
        .unwrap();
    assert_eq!(
        file.text,
        "class Dish:\n    name: str\n    price: int\n    notes: str | None\n"
    );
}

const BRANCHING_TOML: &str = r#"name     = "Branching"
language = "python"

[inputs.kind]
type    = "enum"
choices = ["query", "command"]

[[edits]]
create   = "src/bistro/branch.py"
template = "branch.py.dt"
"#;

const BRANCHING_PY: &str = "{= if kind == \"query\" =}\ndef ask():\n    {= ? answer: block | what it returns =}\n{= else =}\ndef do():\n    pass\n{= end =}\n";

fn branching_plan(kind: &str) -> spellbook::domain::plan::Plan {
    let dir = bistro_project();
    write_generator(
        dir.path(),
        "branching",
        BRANCHING_TOML,
        &[("branch.py.dt", BRANCHING_PY)],
    );
    let rig = PlanRig::new(dir.path());
    rig.plan("branching", &[("kind", kind)]).unwrap()
}

#[test]
fn an_if_on_a_text_input_branches() {
    assert!(
        branching_plan("query").files[0]
            .text
            .starts_with("def ask():\n    raise NotImplementedError")
    );
    assert_eq!(
        branching_plan("command").files[0].text,
        "def do():\n    pass\n"
    );
}

#[test]
fn a_hole_inside_if_is_listed_only_when_rendered() {
    let names = |plan: &spellbook::domain::plan::Plan| -> Vec<String> {
        plan.holes.iter().map(|hole| hole.name.clone()).collect()
    };
    let query = branching_plan("query");
    assert_eq!(names(&query), ["answer"]);
    assert_eq!(query.holes[0].places[0].line, 2);
    assert!(names(&branching_plan("command")).is_empty());
}
