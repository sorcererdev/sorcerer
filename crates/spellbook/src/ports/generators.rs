//! Where the project's generators come from.

use crate::domain::generator::Generator;
use crate::domain::generator_problems::GeneratorProblem;

/// The project's generators, each loaded or the problem that stopped it.
pub trait Generators: Send + Sync {
    /// Every generator, by name; one broken generator does not hide the others.
    fn all(&self) -> Vec<Result<Generator, GeneratorProblem>>;
    /// The generator with this name; `None` when the project has none by that name.
    fn named(&self, id: &str) -> Option<Result<Generator, GeneratorProblem>>;
}
