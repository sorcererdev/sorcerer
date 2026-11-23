//! A generation planned: every file's path and text, every hole, checked (paths, existing
//! files, parsing) before anything is written.

use std::path::Path;
use std::sync::Arc;

use sorcerer_core::domain::facts::parse_errors_of;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::Loaded;
use sorcerer_core::domain::templates::{Piece, fill, pieces};
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::generator::{Edit, EditBody, Generator, OnExists};
use crate::domain::holes::{HoleAt, HoleBook};
use crate::domain::holes_table::placeholder;
use crate::domain::inputs::{Given, Values};
use crate::domain::plan::{
    FileStatus, Plan, PlanStop, PlannedFile, line_count, plan_id, relative_path,
};
use crate::domain::template::{Rendered, render};
use crate::ports::clock::Clock;
use crate::ports::grammars::Grammars;
use crate::usecases::choices_for::{Resolved, resolve_inputs};

/// What planning reads: the project's files, its grammars, its fences and the clock.
pub struct PlanDeps<'a> {
    pub view: &'a dyn CodebaseView,
    pub grammars: &'a dyn Grammars,
    pub fences: &'a Fences,
    pub clock: &'a dyn Clock,
}

/// Plans a generation: the inputs resolved, each edit's path and text rendered (placeholders
/// from the language of the file's own path), each path kept inside the project and off the
/// fences, an existing file skipped or the plan stopped as the edit says, and every new file a
/// language covers parsed. Nothing is written.
///
/// Example: in bistro, `command` with `command=cancel_table` plans `__init__.py` (skipped: it is
/// there), `cancel_table.py` and `cancel_table__test.py`, and the holes `field`, `type` and
/// `handle` with placeholders in the code.
pub fn plan_generation(
    deps: &PlanDeps,
    generator: &Generator,
    given: &Given,
) -> Result<Plan, PlanStop> {
    let values = match resolve_inputs(deps.view, generator, given) {
        Resolved::Ready(values) => values,
        Resolved::Stop(stop) => return Err(PlanStop::Input(stop)),
    };
    let drafts = generator
        .edits
        .iter()
        .map(|edit| draft_file(deps, generator, &values, edit))
        .collect::<Result<Vec<Draft>, PlanStop>>()?;
    let holes = drafts
        .iter()
        .try_fold(HoleBook::default(), |book, draft| {
            book.with_file(&draft.file.path, &draft.holes)
        })
        .map_err(PlanStop::Template)?
        .holes();
    let inputs = values.inputs();
    Ok(Plan {
        id: plan_id(&generator.id, &inputs, &deps.clock.now().to_rfc3339()),
        generator: generator.id.clone(),
        inputs,
        files: drafts.into_iter().map(|draft| draft.file).collect(),
        holes,
    })
}

/// One planned file and the holes it holds.
struct Draft {
    file: PlannedFile,
    holes: Vec<HoleAt>,
}

/// One edit planned: its path kept, an existing file decided, its text rendered and parsed.
fn draft_file(
    deps: &PlanDeps,
    generator: &Generator,
    values: &Values,
    edit: &Edit,
) -> Result<Draft, PlanStop> {
    let path = kept_path(deps.fences, &edit.create, values)?;
    if deps.view.exists(Path::new(&path)) {
        return match edit.on_exists {
            OnExists::Skip => Ok(skipped(path)),
            OnExists::Error => Err(PlanStop::Exists { path }),
        };
    }
    let language = deps
        .grammars
        .for_path(Path::new(&path))
        .map_err(|e| PlanStop::Template(format!("{path}: {e}")))?;
    let rendered = rendered_body(generator, values, edit, &path, language.as_deref())?;
    if let Some(loaded) = &language {
        refuse_a_parse_error(loaded, &rendered.text, edit, &path)?;
    }
    Ok(Draft {
        file: PlannedFile {
            lines: line_count(&rendered.text),
            path,
            status: FileStatus::New,
            text: rendered.text,
        },
        holes: rendered.holes,
    })
}

/// The edit's path filled and kept only when it stays in the project and off the fences.
fn kept_path(fences: &Fences, create: &str, values: &Values) -> Result<String, PlanStop> {
    let filled = fill(create, &values.pairs()).map_err(PlanStop::Template)?;
    let path = relative_path(&filled).map_err(|why| PlanStop::PathRefused {
        path: filled.clone(),
        why,
    })?;
    if !fences.allows_relative(Path::new(&path)) {
        return Err(PlanStop::PathRefused {
            path,
            why: fenced_text(),
        });
    }
    Ok(path)
}

fn fenced_text() -> String {
    "a fenced path (secrets such as .env files, .sorcerer/, .git/ and [model] not_evidence are \
     never written)"
        .into()
}

fn skipped(path: String) -> Draft {
    Draft {
        file: PlannedFile {
            path,
            status: FileStatus::Skipped,
            text: String::new(),
            lines: 0,
        },
        holes: Vec::new(),
    }
}

/// The file's text: a text edit as it is, a template rendered with the placeholders of the
/// language the path belongs to.
fn rendered_body(
    generator: &Generator,
    values: &Values,
    edit: &Edit,
    path: &str,
    language: Option<&Loaded>,
) -> Result<Rendered, PlanStop> {
    let template = match &edit.body {
        EditBody::Text(text) => {
            return Ok(Rendered {
                text: text.clone(),
                holes: Vec::new(),
            });
        }
        EditBody::Template(name) => &generator.templates[name],
    };
    let Some(loaded) = language else {
        if let Some(hole) = first_placeholder_hole(template, generator) {
            return Err(PlanStop::NoLanguageForHole {
                path: path.into(),
                hole,
            });
        }
        return render(template, values, &generator.holes, &|_, _, _| {
            Err(String::new())
        })
        .map_err(|why| PlanStop::Template(format!("{path}: {why}")));
    };
    let placeholder_of = |kind: &str, name: &str, instruction: &str| {
        placeholder(&loaded.language, kind, name, instruction).map_err(|e| e.to_string())
    };
    render(template, values, &generator.holes, &placeholder_of)
        .map_err(|why| PlanStop::Template(format!("{path}: {why}")))
}

/// The first hole in a template that no `[holes.X]` fills.
fn first_placeholder_hole(template: &str, generator: &Generator) -> Option<String> {
    pieces(template)
        .ok()?
        .into_iter()
        .find_map(|piece| match piece {
            Piece::Hole { name, .. } if !generator.holes.contains_key(name) => {
                Some(name.to_string())
            }
            _ => None,
        })
}

fn refuse_a_parse_error(
    loaded: &Arc<Loaded>,
    text: &str,
    edit: &Edit,
    path: &str,
) -> Result<(), PlanStop> {
    let errors =
        parse_errors_of(loaded, text).map_err(|e| PlanStop::Template(format!("{path}: {e}")))?;
    match errors.first() {
        None => Ok(()),
        Some(line) => Err(PlanStop::DoesNotParse {
            template: match &edit.body {
                EditBody::Template(name) => format!("templates/{name}"),
                EditBody::Text(_) => "its text".into(),
            },
            file: path.into(),
            line: *line,
        }),
    }
}
