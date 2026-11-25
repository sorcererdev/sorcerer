//! The fixes a finding offers (a query): each fix of its rule with the inputs the finding settles
//! and the ones still to ask. Used by `check --json`, the editor's code actions and `fix`.

use std::collections::BTreeMap;

use serde::Serialize;
use sorcerer_core::ports::view::CodebaseView;

use crate::domain::fix::{FixBuiltins, fix_inputs, fix_label};
use crate::domain::generator::Generator;
use crate::domain::inputs::{Given, InputStop};
use crate::domain::rule::Rule;
use crate::ports::generators::Generators;
use crate::usecases::choices_for::{Resolved, resolve_inputs};

/// One fix offered for a finding: which of the rule's fixes (from 1), what it shows, its
/// generator, the inputs the finding settles and the inputs still to ask.
///
/// Example: `{ index: 1, label: "Use a port the service has", generator: "use-port", inputs:
/// { app: "bistro", service: "booking", kind: "commands", unit: "close_table" }, missing:
/// ["port"] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FixOffer {
    pub index: usize,
    pub label: String,
    pub generator: String,
    pub inputs: BTreeMap<String, String>,
    pub missing: Vec<String>,
}

/// The rule's fixes for a finding in `file` whose node's text is `node_text`, best first. A fix
/// whose generator does not load is left out (the generator's own problem says why).
pub fn fixes_for(
    view: &dyn CodebaseView,
    generators: &dyn Generators,
    rule: &Rule,
    file: &str,
    node_text: &str,
) -> Vec<FixOffer> {
    let slots = rule.files.matches(file).unwrap_or_default();
    let builtins = FixBuiltins::of(file, &slots, node_text);
    rule.fixes
        .iter()
        .enumerate()
        .filter_map(|(at, fix)| {
            let Some(Ok(generator)) = generators.named(&fix.generator) else {
                return None;
            };
            let given = fix_inputs(fix, &builtins).ok()?;
            Some(FixOffer {
                index: at + 1,
                label: fix_label(fix, &generator.name, &builtins).ok()?,
                generator: fix.generator.clone(),
                missing: missing_inputs(view, &generator, &given),
                inputs: given.0,
            })
        })
        .collect()
}

/// The inputs still to ask after the given ones, in the generator's order: each input the
/// project does not decide (an enum with one choice is decided). Found by resolving, and when an
/// input is missing, standing in its first choice (or a name) to find the next.
fn missing_inputs(view: &dyn CodebaseView, generator: &Generator, given: &Given) -> Vec<String> {
    let mut asked = given.clone();
    let mut missing = Vec::new();
    for _ in 0..generator.inputs.len() {
        match resolve_inputs(view, generator, &asked) {
            Resolved::Stop(InputStop::Missing { input, choices, .. }) => {
                let stand_in = choices.first().cloned().unwrap_or_else(|| "x".into());
                asked.0.insert(input.clone(), stand_in);
                missing.push(input);
            }
            _ => break,
        }
    }
    missing
}
