//! The run log: what each step of a review was given and decided, as
//! Markdown text the review adds as it goes. Only the review's own thread
//! writes to it.

pub trait RunLog {
    /// Adds the text to the end of the log.
    fn put(&self, text: &str);
}

/// The log that keeps nothing (the eval, and tests that do not read it).
pub struct Quiet;

impl RunLog for Quiet {
    fn put(&self, _: &str) {}
}

/// A closure can stand in as a log, as the tests and the eval's collector do.
impl<F: Fn(&str)> RunLog for F {
    fn put(&self, text: &str) {
        self(text)
    }
}
