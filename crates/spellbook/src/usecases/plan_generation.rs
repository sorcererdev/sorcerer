//! A generation planned: every file's path and text, every change to a file that is there, every
//! hole, checked (paths, existing files, anchors, parsing) before anything is written.

use std::path::Path;
use std::sync::Arc;

use regex::Regex;
use sorcerer_core::domain::facts::parse_errors_of;
use sorcerer_core::domain::fences::Fences;
use sorcerer_core::domain::language::Loaded;
use sorcerer_core::domain::templates::{Piece, fill, pieces};
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::anchor::{Anchor, Resolution, resolution_text};
use crate::domain::change::{apply_changes, change_at};
use crate::domain::generator::{Edit, EditBody, Generator, OnExists, OnMissing};
use crate::domain::holes::{HoleAt, HoleBook};
use crate::domain::holes_table::placeholder;
use crate::domain::inputs::{Given, Values};
use crate::domain::plan::{
    Applied, FileStatus, Plan, PlanStop, PlannedFile, line_count, plan_id, relative_path,
};
use crate::domain::template::{Rendered, render};
use crate::ports::anchors::Anchors;
use crate::ports::clock::Clock;
use crate::ports::generators::Generators;
use crate::ports::grammars::Grammars;
use crate::usecases::choices_for::{Resolved, resolve_inputs};
use crate::usecases::planned_view::PlannedView;

/// What planning reads: the project's files, its grammars and anchors, its fences and the clock.
pub struct PlanDeps<'a> {
    pub view: &'a dyn CodebaseView,
    pub grammars: &'a dyn Grammars,
    pub anchors: &'a dyn Anchors,
    /// Where a `[[uses]]` finds the generator it plans.
    pub generators: &'a dyn Generators,
    pub fences: &'a Fences,
    pub clock: &'a dyn Clock,
}

/// Plans a generation: the inputs resolved, each edit's path and text rendered (placeholders
/// from the language of the file's own path), each path kept inside the project and off the
/// fences, an existing file skipped or the plan stopped as the edit says, each insert placed by
/// its phrase (or left out when its `unless` holds), and every new or changed file a language
/// covers parsed. A generator with inserts whose every edit is in place already is
/// `NothingToDo`. Nothing is written.
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
        .enumerate()
        .try_fold(Vec::new(), |drafts, (number, edit)| {
            draft_edit(deps, generator, &values, drafts, number + 1, edit)
        })?;
    let drafts: Vec<Draft> = drafts.into_iter().map(Draft::finished).collect();
    let holes = drafts
        .iter()
        .try_fold(HoleBook::default(), |book, draft| {
            book.with_file(&draft.file.path, &draft.holes)
        })
        .map_err(PlanStop::Template)?
        .holes();
    let own = (drafts.into_iter().map(|draft| draft.file).collect(), holes);
    let (files, holes) = generator
        .uses
        .iter()
        .try_fold(own, |so_far, used| with_used(deps, &values, so_far, used))?;
    let nothing_new = files.iter().all(|file: &PlannedFile| {
        matches!(file.status, FileStatus::Skipped | FileStatus::Unchanged)
    });
    if (has_inserts(generator) || !generator.uses.is_empty()) && nothing_new {
        return Err(PlanStop::NothingToDo(Applied::Generation));
    }
    let inputs = values.inputs();
    Ok(Plan {
        id: plan_id(&generator.id, &inputs, &deps.clock.now().to_rfc3339()),
        generator: generator.id.clone(),
        inputs,
        files,
        holes,
        fix: None,
        notes: Vec::new(),
    })
}

/// The plan so far with one `[[uses]]` planned after it, over a view of the project with the
/// plan so far laid on it: the used plan's new files appended, a file it changes again kept as
/// one change from the disk's text, its holes appended. A used generator with nothing to do adds
/// nothing.
fn with_used(
    deps: &PlanDeps,
    values: &Values,
    (mut files, mut holes): (Vec<PlannedFile>, Vec<crate::domain::holes::Hole>),
    used: &crate::domain::generator::Use,
) -> Result<(Vec<PlannedFile>, Vec<crate::domain::holes::Hole>), PlanStop> {
    let generator = match deps.generators.named(&used.generator) {
        Some(Ok(generator)) => generator,
        Some(Err(problem)) => return Err(PlanStop::Generator(problem.to_string())),
        None => {
            return Err(PlanStop::Generator(format!(
                "the project has no generator `{}`",
                used.generator
            )));
        }
    };
    let given = used
        .with
        .iter()
        .map(|(name, template)| Ok((name.clone(), fill(template, &values.pairs())?)))
        .collect::<Result<_, String>>()
        .map(Given)
        .map_err(PlanStop::Template)?;
    let planned = files
        .iter()
        .filter(|file| matches!(file.status, FileStatus::New | FileStatus::Changed))
        .map(|file| (file.path.clone(), file.text.clone()))
        .collect();
    let view = PlannedView::new(deps.view, planned);
    let over_the_plan = PlanDeps {
        view: &view,
        grammars: deps.grammars,
        anchors: deps.anchors,
        generators: deps.generators,
        fences: deps.fences,
        clock: deps.clock,
    };
    let plan = match plan_generation(&over_the_plan, &generator, &given) {
        Ok(plan) => plan,
        Err(PlanStop::NothingToDo(_)) => return Ok((files, holes)),
        Err(stop) => return Err(stop),
    };
    for file in plan.files {
        match files.iter().position(|known| known.path == file.path) {
            Some(at) if file.status == FileStatus::Changed => {
                let known = &files[at];
                files[at] = match known.status {
                    FileStatus::New => PlannedFile::new_file(file.path, file.text),
                    FileStatus::Changed => {
                        PlannedFile::changed(file.path, known.before.clone(), file.text)
                    }
                    FileStatus::Skipped | FileStatus::Unchanged => file,
                };
            }
            Some(_) => {}
            None => files.push(file),
        }
    }
    holes.extend(plan.holes);
    Ok((files, holes))
}

/// One planned file and the holes it holds.
struct Draft {
    file: PlannedFile,
    holes: Vec<HoleAt>,
}

impl Draft {
    /// A file that is there, read for an insert: unchanged until an insert lands.
    fn read(path: String, text: String) -> Draft {
        let mut file = PlannedFile::left(path, FileStatus::Unchanged);
        file.before = text.clone();
        file.text = text;
        Draft {
            file,
            holes: Vec::new(),
        }
    }

    /// A read file with its changes and diff worked out, or left unchanged.
    fn finished(self) -> Draft {
        if self.file.status != FileStatus::Changed {
            return self;
        }
        Draft {
            file: PlannedFile::changed(self.file.path, self.file.before, self.file.text),
            holes: self.holes,
        }
    }
}

fn has_inserts(generator: &Generator) -> bool {
    generator
        .edits
        .iter()
        .any(|edit| matches!(edit, Edit::Insert { .. }))
}

/// The drafts with one more edit planned: a new file, or lines put in a file.
fn draft_edit(
    deps: &PlanDeps,
    generator: &Generator,
    values: &Values,
    mut drafts: Vec<Draft>,
    number: usize,
    edit: &Edit,
) -> Result<Vec<Draft>, PlanStop> {
    match edit {
        Edit::Create { .. } => drafts.push(draft_file(deps, generator, values, edit)?),
        Edit::Insert { .. } => {
            let insert = Insert {
                deps,
                generator,
                values,
                number,
                edit,
            };
            insert.land_in(&mut drafts)?;
        }
    }
    Ok(drafts)
}

/// One `insert` edit being planned.
struct Insert<'a> {
    deps: &'a PlanDeps<'a>,
    generator: &'a Generator,
    values: &'a Values,
    number: usize,
    edit: &'a Edit,
}

impl Insert<'_> {
    /// Puts the edit's lines in its file's draft: (1) the file, as planned so far or as it is on
    /// disk; (2) skipped when `when` does not resolve or `unless` holds; (3) the place, the first
    /// phrase of `at` that resolves; (4) the body rendered and placed, its holes counted on the
    /// file's lines; (5) the file still parses.
    fn land_in(&self, drafts: &mut Vec<Draft>) -> Result<(), PlanStop> {
        let Edit::Insert {
            file,
            at,
            unless,
            when,
            on_missing,
            ..
        } = self.edit
        else {
            return Ok(());
        };
        let path = kept_path(self.deps.fences, file, self.values)?;
        let Some(index) = self.draft_of(drafts, &path, *on_missing)? else {
            return Ok(());
        };
        let text = drafts[index].file.text.clone();
        if !self.wanted(&path, &text, when.as_deref(), unless.as_deref())? {
            return Ok(());
        }
        let Some(place) = self.place(&path, &text, at, *on_missing)? else {
            return Ok(());
        };
        let language = self
            .deps
            .grammars
            .for_path(Path::new(&path))
            .map_err(|e| PlanStop::Template(format!("{path}: {e}")))?;
        let template = match self.edit.body() {
            EditBody::Text(text) => text.as_str(),
            EditBody::Template(name) => &self.generator.templates[name],
        };
        let rendered = render_in_language(
            self.generator,
            self.values,
            template,
            &path,
            language.as_deref(),
        )?;
        let change = change_at(&text, &place, &rendered.text);
        let changed = apply_changes(&text, std::slice::from_ref(&change))
            .map_err(|why| self.stop(&path, why))?;
        if let Some(loaded) = &language {
            self.refuse_a_new_parse_error(loaded, &text, &changed, &path)?;
        }
        let draft = &mut drafts[index];
        let shift = line_count(&change.new) as i64 - (change.to_row - change.from_row) as i64;
        for hole in &mut draft.holes {
            if hole.line as usize > change.to_row {
                hole.line = u32::try_from(hole.line as i64 + shift).unwrap_or(hole.line);
            }
        }
        let first_line = u32::try_from(change.from_row).unwrap_or(u32::MAX);
        draft
            .holes
            .extend(rendered.holes.into_iter().map(|mut hole| {
                hole.line += first_line;
                hole
            }));
        if draft.file.status == FileStatus::Unchanged {
            draft.file.status = FileStatus::Changed;
        }
        draft.file.text = changed;
        Ok(())
    }

    /// The draft of the file the edit changes: one planned already, or the file read now. `None`
    /// when the file is not there and the edit says `on_missing = "skip"`.
    fn draft_of(
        &self,
        drafts: &mut Vec<Draft>,
        path: &str,
        on_missing: OnMissing,
    ) -> Result<Option<usize>, PlanStop> {
        let planned = drafts
            .iter()
            .position(|draft| draft.file.path == path && draft.file.status != FileStatus::Skipped);
        if planned.is_some() {
            return Ok(planned);
        }
        if !self.deps.view.exists(Path::new(path)) {
            return match on_missing {
                OnMissing::Skip => Ok(None),
                OnMissing::Error => Err(self.stop(path, "the file is not there".into())),
            };
        }
        let text = self
            .deps
            .view
            .text(Path::new(path))
            .map_err(|e| self.stop(path, e.to_string()))?;
        drafts.push(Draft::read(path.to_string(), text));
        Ok(Some(drafts.len() - 1))
    }

    /// Whether the edit is still to make: its `when` phrase resolves (when it has one) and no
    /// line matches its `unless`.
    fn wanted(
        &self,
        path: &str,
        text: &str,
        when: Option<&str>,
        unless: Option<&str>,
    ) -> Result<bool, PlanStop> {
        if let Some(phrase) = when {
            let anchor = self.anchor(path, phrase)?;
            if !matches!(self.resolve(path, text, &anchor)?, Resolution::At(_)) {
                return Ok(false);
            }
        }
        let Some(pattern) = unless else {
            return Ok(true);
        };
        let filled = fill(pattern, &self.values.pairs()).map_err(PlanStop::Template)?;
        let regex = Regex::new(&filled).map_err(|e| {
            self.stop(
                path,
                format!("unless: the regex `{filled}` does not compile ({e})"),
            )
        })?;
        Ok(!text.lines().any(|line| regex.is_match(line)))
    }

    /// The first of the `at` phrases that resolves; when none does, the plan stops on the first
    /// one's miss, or the edit is left out (`on_missing = "skip"`).
    fn place(
        &self,
        path: &str,
        text: &str,
        at: &[String],
        on_missing: OnMissing,
    ) -> Result<Option<crate::domain::anchor::Place>, PlanStop> {
        let mut first_miss: Option<String> = None;
        for phrase in at {
            let anchor = self.anchor(path, phrase)?;
            match self.resolve(path, text, &anchor)? {
                Resolution::At(place) => return Ok(Some(place)),
                miss => {
                    first_miss
                        .get_or_insert_with(|| resolution_text(&anchor, file_name(path), &miss));
                }
            }
        }
        match on_missing {
            OnMissing::Skip => Ok(None),
            OnMissing::Error => Err(self.stop(path, first_miss.unwrap_or_default())),
        }
    }

    fn anchor(&self, path: &str, phrase: &str) -> Result<Anchor, PlanStop> {
        let filled = fill(phrase, &self.values.pairs()).map_err(PlanStop::Template)?;
        Anchor::parse(&filled).map_err(|why| self.stop(path, why))
    }

    fn resolve(&self, path: &str, text: &str, anchor: &Anchor) -> Result<Resolution, PlanStop> {
        self.deps
            .anchors
            .resolve(Path::new(path), text, anchor)
            .map_err(|e| self.stop(path, e.to_string()))
    }

    /// A file that parsed before the edit and does not after it stops the plan; one that had
    /// errors already is not the edit's doing.
    fn refuse_a_new_parse_error(
        &self,
        loaded: &Arc<Loaded>,
        before: &str,
        after: &str,
        path: &str,
    ) -> Result<(), PlanStop> {
        let errors_of = |text: &str| {
            parse_errors_of(loaded, text).map_err(|e| PlanStop::Template(format!("{path}: {e}")))
        };
        if !errors_of(before)?.is_empty() {
            return Ok(());
        }
        match errors_of(after)?.first() {
            None => Ok(()),
            Some(line) => Err(PlanStop::DoesNotParse {
                template: format!("[[edits]] {} insert", self.number),
                file: path.into(),
                line: *line,
            }),
        }
    }

    fn stop(&self, path: &str, why: String) -> PlanStop {
        PlanStop::Anchor {
            edit: format!("[[edits]] {} insert", self.number),
            file: path.into(),
            why,
        }
    }
}

/// The last part of a path: the file's own name.
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// One edit planned: its path kept, an existing file decided, its text rendered and parsed.
fn draft_file(
    deps: &PlanDeps,
    generator: &Generator,
    values: &Values,
    edit: &Edit,
) -> Result<Draft, PlanStop> {
    let path = kept_path(deps.fences, edit.path(), values)?;
    if deps.view.exists(Path::new(&path)) {
        return match edit {
            Edit::Create {
                on_exists: OnExists::Skip,
                ..
            } => Ok(skipped(path)),
            _ => Err(PlanStop::Exists { path }),
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
        file: PlannedFile::new_file(path, rendered.text),
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
        file: PlannedFile::left(path, FileStatus::Skipped),
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
    let template = match edit.body() {
        EditBody::Text(text) => {
            return Ok(Rendered {
                text: text.clone(),
                holes: Vec::new(),
            });
        }
        EditBody::Template(name) => &generator.templates[name],
    };
    render_in_language(generator, values, template, path, language)
}

/// A template rendered with the placeholders of the language the path belongs to; a hole in a
/// file no language covers stops the plan.
fn render_in_language(
    generator: &Generator,
    values: &Values,
    template: &str,
    path: &str,
    language: Option<&Loaded>,
) -> Result<Rendered, PlanStop> {
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
            template: match edit.body() {
                EditBody::Template(name) => format!("templates/{name}"),
                EditBody::Text(_) => "its text".into(),
            },
            file: path.into(),
            line: *line,
        }),
    }
}
