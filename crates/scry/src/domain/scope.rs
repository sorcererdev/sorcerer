//! The scope of a pass: which files it looks at, and which of their units a
//! change adds or alters. A pass is started for files the user names, for
//! the working tree, or for a branch since it left a rev.

use std::path::PathBuf;

use sorcerer_core::domain::fingerprint::Fingerprint;
use sorcerer_core::domain::unit::Unit;

/// Which files a pass looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeRequest {
    /// The files named.
    Files(Vec<PathBuf>),
    /// The files a branch changed since it left the revision (compared with
    /// their merge base).
    Since(String),
    /// The files that differ from HEAD, untracked ones too.
    Working,
}

/// A unit in scope, with the first line of the unit that encloses it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopedUnit {
    pub file: PathBuf,
    pub unit: Unit,
    pub header: Option<String>,
}

/// A file in scope, whether or not any of its units are.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopedFile {
    pub file: PathBuf,
    /// The file's diff against the base; its whole text when that is empty
    /// (untracked, unchanged, or no commit yet).
    pub diff: String,
    /// Every unit name in the file now, for the gone sweep.
    pub units_now: Vec<String>,
    /// The units the same now as at the base, each with its fingerprint, for the put-back
    /// sweep. Empty with `--all` or when there is no base.
    pub same: Vec<(String, Fingerprint)>,
    pub deleted: bool,
}

/// The revision a pass compares the code with, to see what changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base {
    /// `HEAD`, or for `--since` the commit where the branch left.
    Revision(String),
    /// The repository has no commit yet: every file is new, and every unit
    /// in it is in scope.
    NoCommitYet,
}

impl Base {
    /// The revision, unless there is no commit yet.
    pub fn revision(&self) -> Option<&str> {
        match self {
            Base::Revision(rev) => Some(rev),
            Base::NoCommitYet => None,
        }
    }
}

/// What one pass looks at: the files, the units in them it reviews, and the
/// files it left out (and why).
#[derive(Debug, Clone, PartialEq)]
pub struct Scope {
    pub request: ScopeRequest,
    /// What the code is compared with.
    pub base: Base,
    pub units: Vec<ScopedUnit>,
    pub files: Vec<ScopedFile>,
    /// The files considered and left out, each with why (for the run log).
    pub skipped: Vec<(PathBuf, String)>,
}

impl Scope {
    /// A scope for the request, compared with the base, holding nothing
    /// yet.
    pub fn empty(request: ScopeRequest, base: Base) -> Scope {
        Scope {
            request,
            base,
            units: Vec::new(),
            files: Vec::new(),
            skipped: Vec::new(),
        }
    }
}

/// The definition line, trimmed, of the innermost unit that encloses the
/// unit; none when nothing encloses it.
///
/// Example: for the method `Order.cancel`, the header is `class Order:`, so
/// the reviewer sees which class the method belongs to.
pub fn header_of(lines: &[&str], units: &[Unit], unit: &Unit) -> Option<String> {
    let outer = units
        .iter()
        .filter(|m| m.name != unit.name && m.span.contains(&unit.span) && m.span != unit.span)
        .min_by_key(|m| m.span.end - m.span.start)?;
    let line = def_line(lines, units, outer)?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

/// The unit's own name: its name without the name of the unit around it and whatever the
/// language joins the two with (`cancel` in `Order.cancel` or `Order::cancel`).
fn own_name<'u>(units: &[Unit], u: &'u Unit) -> &'u str {
    let parent = units
        .iter()
        .filter(|m| m.name != u.name && m.span.contains(&u.span) && m.span != u.span)
        .min_by_key(|m| m.span.end - m.span.start);
    parent
        .and_then(|parent| u.name.strip_prefix(parent.name.as_str()))
        .map(|rest| rest.trim_start_matches(|c: char| !(c.is_alphanumeric() || c == '_')))
        .filter(|own| !own.is_empty())
        .unwrap_or(&u.name)
}

/// The unit's definition line: the first line of its span, before its body,
/// that holds its own name (so a decorator above it is passed over); else
/// the span's first line.
fn def_line<'t>(lines: &[&'t str], units: &[Unit], u: &Unit) -> Option<&'t str> {
    let own = own_name(units, u);
    let last = u.body.map_or(u.span.end, |b| b.start.max(u.span.start));
    let at = |n: u32| lines.get(n.checked_sub(1)? as usize).copied();
    (u.span.start..=last)
        .filter_map(at)
        .find(|l| !own.is_empty() && l.contains(own))
        .or_else(|| at(u.span.start))
}
