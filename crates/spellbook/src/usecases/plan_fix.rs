//! A fix planned (a query): the finding found again, its fix's generator planned, and the found
//! code replaced, all in one plan. Nothing is written; `apply_plan` writes it.

use std::path::Path;

use sorcerer_core::domain::facts::parse_errors_of;
use sorcerer_core::domain::language::Loaded;

use crate::domain::anchor::indent_of;
use crate::domain::change::{TextChange, apply_changes, with_line_ending};
use crate::domain::finding::{RawFinding, fingerprint};
use crate::domain::fix::{Fix, FixBuiltins, fix_inputs, fix_label, with_instructions_filled};
use crate::domain::generator::Generator;
use crate::domain::holes::{HoleAt, HoleBook};
use crate::domain::holes_table::placeholder;
use crate::domain::inputs::{Given, Values};
use crate::domain::plan::{Applied, FileStatus, FixOrigin, Plan, PlanStop, PlannedFile, plan_id};
use crate::domain::rule::Rule;
use crate::domain::template::render;
use crate::ports::generators::Generators;
use crate::ports::rule_runner::RuleRunner;
use crate::usecases::choices_for::{Resolved, resolve_inputs};
use crate::usecases::plan_generation::{PlanDeps, plan_generation};

/// What planning a fix reads: what planning a generation reads, the rules and their runner, and
/// the generators.
pub struct FixDeps<'a> {
    pub plan: PlanDeps<'a>,
    pub runner: &'a dyn RuleRunner,
    pub rules: &'a [Rule],
    pub generators: &'a dyn Generators,
}

/// The finding a fix answers: its file (relative to the root, with `/`), its line (from 1), its
/// rule when two rules fire on the line, and which of the rule's fixes (from 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingAt {
    pub file: String,
    pub line: u32,
    pub rule: Option<String>,
    pub index: usize,
}

/// Plans a fix: (1) the finding found again on its line in the file as it is now, else
/// `FindingGone` (or `NothingToDo` when the fix's edits are all in place: it was applied); (2) the
/// fix picked, its `with` filled from the finding and the person's `given` laid over it; (3) the
/// generator planned with those inputs; (4) the found code's lines replaced by the first
/// `replace` case whose node kind encloses it; (5) one plan, the replaced file's changes composed
/// with the generator's; (6) the replaced file still parses.
///
/// Example: on bistro's `close_table.py:4` (`import sqlite3`) with `port=table_repository`, the
/// plan changes `close_table.py` (the import swapped for the port's, `__init__` taking the port)
/// and `close_table__test.py` (the fake passed in).
pub fn plan_fix(deps: &FixDeps, at: &FindingAt, given: &Given) -> Result<Plan, PlanStop> {
    let text = deps
        .plan
        .view
        .text(Path::new(&at.file))
        .map_err(|_| gone(at))?;
    let Some((rule, found)) = finding_on_the_line(deps, at, &text)? else {
        return Err(gone_or_already_applied(deps, at, given));
    };
    let fix = the_fix(rule, at.index)?;
    let generator = the_generator(deps.generators, &fix.generator)?;
    let builtins = FixBuiltins::of(
        &at.file,
        &rule.files.matches(&at.file).unwrap_or_default(),
        &found.node_text,
    );
    let inputs = laid_over(
        fix_inputs(fix, &builtins).map_err(PlanStop::Template)?,
        given,
    );
    let plan = generation_or_nothing(&deps.plan, &generator, &inputs)?;
    let plan = match replacement(deps, at, &text, &found, fix, &generator, &inputs)? {
        Some(replacement) => with_replacement(plan, &at.file, &text, replacement)?,
        None => plan,
    };
    refuse_a_new_parse_error(deps, &at.file, &text, &plan)?;
    Ok(Plan {
        fix: Some(FixOrigin {
            rule: rule.id.clone(),
            file: at.file.clone(),
            line: at.line,
            fingerprint: fingerprint(&rule.id, &found.node_text),
            index: at.index,
            label: fix_label(fix, &generator.name, &builtins).map_err(PlanStop::Template)?,
        }),
        ..plan
    })
}

fn gone(at: &FindingAt) -> PlanStop {
    PlanStop::FindingGone {
        file: at.file.clone(),
        line: at.line,
        rule: at.rule.clone().unwrap_or_else(|| "spellbook".into()),
    }
}

/// The first finding on the line of a rule that looks at the file (the named rule, when given).
fn finding_on_the_line<'r>(
    deps: &'r FixDeps,
    at: &FindingAt,
    text: &str,
) -> Result<Option<(&'r Rule, RawFinding)>, PlanStop> {
    let rules: Vec<&Rule> = deps
        .rules
        .iter()
        .filter(|rule| rule.applies_to(&at.file))
        .filter(|rule| at.rule.as_ref().is_none_or(|id| *id == rule.id))
        .collect();
    let found = deps
        .runner
        .findings_in(Path::new(&at.file), text, &rules)
        .map_err(|e| PlanStop::Template(e.to_string()))?;
    Ok(rules.iter().find_map(|rule| {
        found
            .iter()
            .find(|finding| finding.rule == rule.id && finding.line == at.line)
            .map(|finding| (*rule, finding.clone()))
    }))
}

/// A finding that is not there: the fix already applied when its generator has nothing left to
/// do, else gone.
fn gone_or_already_applied(deps: &FixDeps, at: &FindingAt, given: &Given) -> PlanStop {
    let rule = deps
        .rules
        .iter()
        .filter(|rule| rule.applies_to(&at.file) && !rule.fixes.is_empty())
        .find(|rule| at.rule.as_ref().is_none_or(|id| *id == rule.id));
    let Some(rule) = rule else {
        return gone(at);
    };
    let Ok(fix) = the_fix(rule, at.index) else {
        return gone(at);
    };
    let Ok(generator) = the_generator(deps.generators, &fix.generator) else {
        return gone(at);
    };
    let builtins = FixBuiltins::of(
        &at.file,
        &rule.files.matches(&at.file).unwrap_or_default(),
        "",
    );
    let Ok(inputs) = fix_inputs(fix, &builtins) else {
        return gone(at);
    };
    match plan_generation(&deps.plan, &generator, &laid_over(inputs, given)) {
        Err(PlanStop::NothingToDo(_)) => PlanStop::NothingToDo(Applied::Fix),
        _ => PlanStop::FindingGone {
            file: at.file.clone(),
            line: at.line,
            rule: rule.id.clone(),
        },
    }
}

fn the_fix(rule: &Rule, index: usize) -> Result<&Fix, PlanStop> {
    index
        .checked_sub(1)
        .and_then(|at| rule.fixes.get(at))
        .ok_or_else(|| {
            PlanStop::Generator(format!(
                "rule {} has no fix {index} (it has {})",
                rule.id,
                rule.fixes.len()
            ))
        })
}

fn the_generator(generators: &dyn Generators, id: &str) -> Result<Generator, PlanStop> {
    match generators.named(id) {
        Some(Ok(generator)) => Ok(generator),
        Some(Err(problem)) => Err(PlanStop::Generator(problem.to_string())),
        None => Err(PlanStop::Generator(format!(
            "the project has no generator `{id}`"
        ))),
    }
}

/// The fix's inputs with the person's laid over them: what the person gave wins.
fn laid_over(from_fix: Given, given: &Given) -> Given {
    let mut inputs = from_fix.0;
    inputs.extend(given.0.clone());
    Given(inputs)
}

/// The generator's plan, or an empty one when its edits are all in place already (the found code
/// may still be replaced).
fn generation_or_nothing(
    deps: &PlanDeps,
    generator: &Generator,
    inputs: &Given,
) -> Result<Plan, PlanStop> {
    match plan_generation(deps, generator, inputs) {
        Err(PlanStop::NothingToDo(_)) => {
            let values = values_of(deps, generator, inputs)?;
            let inputs = values.inputs();
            Ok(Plan {
                id: plan_id(&generator.id, &inputs, &deps.clock.now().to_rfc3339()),
                generator: generator.id.clone(),
                inputs,
                files: Vec::new(),
                holes: Vec::new(),
                fix: None,
            })
        }
        planned => planned,
    }
}

fn values_of(deps: &PlanDeps, generator: &Generator, inputs: &Given) -> Result<Values, PlanStop> {
    match resolve_inputs(deps.view, generator, inputs) {
        Resolved::Ready(values) => Ok(values),
        Resolved::Stop(stop) => Err(PlanStop::Input(stop)),
    }
}

/// The found code's whole lines and what they become, rows counted in the file as it is.
struct Replacement {
    change: TextChange,
    holes: Vec<HoleAt>,
}

/// The first `replace` case whose node kind encloses the finding: that node's lines become the
/// case's text, rendered over the generator's values and `finding` (that node's text), indented
/// like the node.
/// `None` for a fix with no `replace`.
fn replacement(
    deps: &FixDeps,
    at: &FindingAt,
    text: &str,
    found: &RawFinding,
    fix: &Fix,
    generator: &Generator,
    inputs: &Given,
) -> Result<Option<Replacement>, PlanStop> {
    if fix.replace.is_empty() {
        return Ok(None);
    }
    let file = Path::new(&at.file);
    let mut chosen = None;
    for case in &fix.replace {
        let node = deps
            .runner
            .enclosing(file, text, found.span, &case.enclosing)
            .map_err(|e| PlanStop::Template(e.to_string()))?;
        if let Some(node) = node {
            chosen = Some((case, node));
            break;
        }
    }
    let Some((case, node)) = chosen else {
        return Err(PlanStop::NoReplaceCase {
            kind: found.kind.clone(),
        });
    };
    let values = values_of(&deps.plan, generator, inputs)?;
    // In a replace text, `finding` is the code being replaced (the whole statement around a
    // matched call), which is what the person reads in the hole's instruction.
    let replaced = &text[node.start_byte..node.end_byte];
    let one_line = replaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut pairs: Vec<(String, String)> = values
        .pairs()
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    pairs.push(("finding".into(), one_line));
    let borrowed: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(n, v)| (n.as_str(), v.as_str()))
        .collect();
    let template =
        with_instructions_filled(&case.text, &borrowed, replaced).map_err(PlanStop::Template)?;
    let language = deps
        .plan
        .grammars
        .for_path(file)
        .map_err(|e| PlanStop::Template(e.to_string()))?;
    let placeholder_of = |kind: &str, name: &str, instruction: &str| match &language {
        Some(loaded) => {
            placeholder(&loaded.language, kind, name, instruction).map_err(|e| e.to_string())
        }
        None => Err(format!(
            "no language covers {}, so the hole {name} has no placeholder",
            at.file
        )),
    };
    let rendered = render(
        &template,
        &Values::of_inputs(pairs),
        &generator.holes,
        &placeholder_of,
    )
    .map_err(|why| PlanStop::Template(format!("{}: {why}", at.file)))?;
    let old_lines: Vec<&str> = text
        .split_inclusive('\n')
        .skip(node.start_row)
        .take(node.end_row - node.start_row + 1)
        .collect();
    let indent = indent_of(old_lines.first().copied().unwrap_or(""));
    Ok(Some(Replacement {
        change: TextChange {
            from_row: node.start_row,
            to_row: node.end_row + 1,
            old: old_lines.concat(),
            new: indented_lines(&rendered.text, &indent, text),
        },
        holes: rendered.holes,
    }))
}

/// The rendered text as whole lines indented by `indent`, in the file's line ending; nothing for
/// an empty text (the lines are deleted).
fn indented_lines(rendered: &str, indent: &str, file: &str) -> String {
    let rendered = rendered.trim_end_matches('\n');
    if rendered.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = rendered
        .split('\n')
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{indent}{}", line.trim_start())
            }
        })
        .collect();
    with_line_ending(&format!("{}\n", lines.join("\n")), file)
}

/// The plan with the found code replaced in its file: the file as the generator planned it (or as
/// it is, when the generator does not touch it), the replacement moved past the generator's
/// changes above it; an overlap with one of them stops the plan.
fn with_replacement(
    mut plan: Plan,
    path: &str,
    disk_text: &str,
    replacement: Replacement,
) -> Result<Plan, PlanStop> {
    let index = plan
        .files
        .iter()
        .position(|file| file.path == path && file.status == FileStatus::Changed);
    let (current, earlier) = match index {
        Some(index) => (
            plan.files[index].text.clone(),
            plan.files[index].changes.clone(),
        ),
        None => (disk_text.to_string(), Vec::new()),
    };
    let change = &replacement.change;
    let mut shift: i64 = 0;
    for other in &earlier {
        if other.to_row <= change.from_row {
            shift += rows_of(&other.new) - (other.to_row - other.from_row) as i64;
        } else if other.from_row < change.to_row {
            return Err(PlanStop::Anchor {
                edit: "the fix's replace".into(),
                file: path.into(),
                why: format!(
                    "an edit of the generator changes line {} too",
                    other.from_row + 1
                ),
            });
        }
    }
    let moved = TextChange {
        from_row: (change.from_row as i64 + shift) as usize,
        to_row: (change.to_row as i64 + shift) as usize,
        old: change.old.clone(),
        new: change.new.clone(),
    };
    let after =
        apply_changes(&current, std::slice::from_ref(&moved)).map_err(|why| PlanStop::Anchor {
            edit: "the fix's replace".into(),
            file: path.into(),
            why,
        })?;
    let delta = rows_of(&moved.new) - (moved.to_row - moved.from_row) as i64;
    let mut holes = plan.holes;
    for hole in &mut holes {
        for place in &mut hole.places {
            if place.file == path && place.line as usize > moved.to_row {
                place.line = (place.line as i64 + delta) as u32;
            }
        }
    }
    let first_line = u32::try_from(moved.from_row).unwrap_or(u32::MAX);
    let new_holes: Vec<HoleAt> = replacement
        .holes
        .into_iter()
        .map(|mut hole| {
            hole.line += first_line;
            hole
        })
        .collect();
    plan.holes = HoleBook::of(holes)
        .with_file(path, &new_holes)
        .map_err(PlanStop::Template)?
        .holes();
    let changed = PlannedFile::changed(path.into(), disk_text.into(), after);
    match index {
        Some(index) => plan.files[index] = changed,
        None => plan.files.push(changed),
    }
    Ok(plan)
}

fn rows_of(text: &str) -> i64 {
    text.split_inclusive('\n').count() as i64
}

/// The replaced file parses, when it parsed before the fix.
fn refuse_a_new_parse_error(
    deps: &FixDeps,
    path: &str,
    before: &str,
    plan: &Plan,
) -> Result<(), PlanStop> {
    let Some(file) = plan.files.iter().find(|file| file.path == path) else {
        return Ok(());
    };
    let language = deps
        .plan
        .grammars
        .for_path(Path::new(path))
        .map_err(|e| PlanStop::Template(e.to_string()))?;
    let Some(loaded) = language else {
        return Ok(());
    };
    let errors_of = |text: &str| -> Result<Vec<u32>, PlanStop> {
        parse_errors_of(&loaded as &Loaded, text).map_err(|e| PlanStop::Template(e.to_string()))
    };
    if !errors_of(before)?.is_empty() {
        return Ok(());
    }
    match errors_of(&file.text)?.first() {
        None => Ok(()),
        Some(line) => Err(PlanStop::DoesNotParse {
            template: "the fix's replace".into(),
            file: path.into(),
            line: *line,
        }),
    }
}
