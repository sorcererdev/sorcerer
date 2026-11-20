//! The issue types of a finding: a name, the sentence the review prompt gives it,
//! whether it is shown by default, and how a finding of it is verified.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// Its evidence citations must check out.
    Citations,
    /// Citations, then the claim checker's own call.
    CitationsThenClaim,
}

#[derive(Debug)]
pub struct IssueType {
    pub name: &'static str,
    pub sentence: &'static str,
    pub shown: bool,
    pub verify: Verify,
}

pub static ISSUE_TYPES: &[IssueType] = &[
    IssueType {
        name: "bug",
        sentence: "the code does the wrong thing.",
        shown: true,
        verify: Verify::CitationsThenClaim,
    },
    IssueType {
        name: "assumption",
        sentence: "the code relies on something about other code that is not so.",
        shown: true,
        verify: Verify::CitationsThenClaim,
    },
    IssueType {
        name: "convention",
        sentence: "the code breaks a rule in the steering file.",
        shown: true,
        verify: Verify::Citations,
    },
    IssueType {
        name: "design",
        sentence: "readability, maintainability, testability or architecture with no rule behind it.",
        shown: false,
        verify: Verify::Citations,
    },
];

pub fn issue_type(name: &str) -> Option<&'static IssueType> {
    ISSUE_TYPES.iter().find(|k| k.name == name)
}

#[allow(clippy::too_many_lines)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_issue_types_by_name() {
        assert_eq!(ISSUE_TYPES.len(), 4);
        assert_eq!(
            issue_type("bug").unwrap().verify,
            Verify::CitationsThenClaim
        );
        assert!(!issue_type("design").unwrap().shown);
        assert!(issue_type("style").is_none());
    }
}
