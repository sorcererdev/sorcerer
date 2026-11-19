//! The run log: each step's inputs and decisions, as Markdown text a use
//! case puts as it goes. Called on the use case's thread only.

pub trait RunLog {
    fn put(&self, text: &str);
}

/// The log that keeps nothing (the eval, and tests that do not read it).
pub struct Quiet;

impl RunLog for Quiet {
    fn put(&self, _: &str) {}
}

/// A closure is a log (the tests', and the eval's collector).
impl<F: Fn(&str)> RunLog for F {
    fn put(&self, text: &str) {
        self(text)
    }
}
