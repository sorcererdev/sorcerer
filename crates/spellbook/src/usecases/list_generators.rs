//! The generators of a project, as `spellbook list` and the editor's picker show them.

use serde::Serialize;

use crate::domain::generator::Generator;
use crate::domain::generator_problems::GeneratorProblem;
use crate::ports::generators::Generators;

/// A generator in a line: its name, what it makes, and the inputs it asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GeneratorSummary {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub inputs: Vec<InputSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InputSummary {
    pub name: String,
    /// `ident`, `enum` or `text`.
    #[serde(rename = "type")]
    pub kind: String,
    pub prompt: String,
    pub optional: bool,
}

/// Every generator of the project, by name, each summarised or the problem that stopped it.
pub fn list_generators(
    generators: &dyn Generators,
) -> Vec<Result<GeneratorSummary, GeneratorProblem>> {
    generators
        .all()
        .into_iter()
        .map(|loaded| loaded.map(|generator| summary_of(&generator)))
        .collect()
}

fn summary_of(generator: &Generator) -> GeneratorSummary {
    GeneratorSummary {
        id: generator.id.clone(),
        name: generator.name.clone(),
        summary: generator.summary.clone(),
        inputs: generator
            .inputs
            .iter()
            .map(|(name, input)| InputSummary {
                name: name.clone(),
                kind: input.kind.word().into(),
                prompt: input.prompt.clone(),
                optional: input.optional,
            })
            .collect(),
    }
}
