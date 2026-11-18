//! The kinds of finding: a name, the sentence the find prompt gives it,
//! whether it is shown by default, and how a finding of it is verified.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// Its evidence citations must check out.
    Citations,
    /// Citations, then the claim checker's own call.
    CitationsThenClaim,
}

#[derive(Debug)]
pub struct Kind {
    pub name: &'static str,
    pub sentence: &'static str,
    pub shown: bool,
    pub verify: Verify,
}

pub static KINDS: &[Kind] = &[
    Kind {
        name: "bug",
        sentence: "the code does the wrong thing.",
        shown: true,
        verify: Verify::CitationsThenClaim,
    },
    Kind {
        name: "assumption",
        sentence: "the code relies on something about other code that is not so.",
        shown: true,
        verify: Verify::CitationsThenClaim,
    },
    Kind {
        name: "convention",
        sentence: "the code breaks a rule in the steering file.",
        shown: true,
        verify: Verify::Citations,
    },
    Kind {
        name: "design",
        sentence: "readability, maintainability, testability or architecture with no rule behind it.",
        shown: false,
        verify: Verify::Citations,
    },
];

pub fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_kinds_by_name() {
        assert_eq!(KINDS.len(), 4);
        assert_eq!(kind("bug").unwrap().verify, Verify::CitationsThenClaim);
        assert!(!kind("design").unwrap().shown);
        assert!(kind("style").is_none());
    }
}
