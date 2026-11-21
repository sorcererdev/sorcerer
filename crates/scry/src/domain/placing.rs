//! Where a stored finding sits in its file as the file is now.
//!
//! The store keeps the line a finding had when it was reviewed; the file has moved on since.
//! A finding follows its unit: it keeps its distance from the unit's first line, and it is
//! stale when the unit's code is no longer what was reviewed.

use sorcerer_core::domain::unit::Unit;

use crate::domain::finding::Finding;

/// A finding's line in the file as it is now (1-based), and whether its unit changed since
/// it was reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub line: u32,
    pub stale: bool,
}

/// Where a finding sits among the file's units now, or `None` when its unit is gone.
///
/// 1. A finding with no unit (about the file as a whole) keeps its stored line, moved up to
///    the last line when the file got shorter, and is never stale.
/// 2. Otherwise its unit is the one with the stored name; of two with that name, the one at
///    the stored fingerprint, else the first. No unit of that name: `None` (the next pass
///    marks it gone).
/// 3. It keeps its distance from the unit's first line, inside the unit's lines.
/// 4. It is stale when the unit's fingerprint differs from the stored one.
///
/// Example: a finding stored on line 12 of a unit that started on line 10, whose unit now
/// starts on line 14 with the same code, sits on line 16 and is not stale. Had the unit
/// shrunk to lines 14-15, it would sit on line 15.
pub fn placed(f: &Finding, units_now: &[Unit], line_count: u32) -> Option<Placed> {
    let Some(name) = &f.place.unit else {
        return Some(Placed {
            line: f.place.lines.start.clamp(1, line_count.max(1)),
            stale: false,
        });
    };
    let unit = its_unit(f, name, units_now)?;
    Some(Placed {
        line: line_in(f, unit),
        stale: f
            .place
            .fingerprint
            .is_some_and(|reviewed| reviewed != unit.fingerprint),
    })
}

/// The unit of this name: the one at the stored fingerprint, else the first.
fn its_unit<'a>(f: &Finding, name: &str, units_now: &'a [Unit]) -> Option<&'a Unit> {
    let named = || units_now.iter().filter(|u| u.name == name);
    named()
        .find(|u| Some(u.fingerprint) == f.place.fingerprint)
        .or_else(|| named().next())
}

/// The unit's first line now plus the finding's distance from the unit's first line when it
/// was reviewed (none when that was not stored), kept inside the unit.
fn line_in(f: &Finding, unit: &Unit) -> u32 {
    let reviewed_start = f.place.unit_start.unwrap_or(f.place.lines.start);
    let distance = i64::from(f.place.lines.start) - i64::from(reviewed_start);
    let line = i64::from(unit.span.start) + distance;
    let inside = line.clamp(i64::from(unit.span.start), i64::from(unit.span.end));
    u32::try_from(inside).unwrap_or(unit.span.start)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use sorcerer_core::domain::fingerprint::Fingerprint;
    use sorcerer_core::domain::unit::{Span, UnitKind};

    use super::*;
    use crate::domain::finding::{FindingId, Place, Scores, Status};

    const REVIEWED: &str = "0123abc";
    const CHANGED: &str = "fedc987";

    fn finding(unit: Option<&str>, line: u32, unit_start: Option<u32>) -> Finding {
        let t: DateTime<Utc> = "2026-11-20T12:00:00Z".parse().unwrap();
        Finding {
            id: FindingId("abc123".into()),
            issue_type: "bug".into(),
            rule: None,
            statement: "s".into(),
            place: Place {
                file: "a.txt".into(),
                unit: unit.map(String::from),
                lines: Span {
                    start: line,
                    end: line,
                },
                unit_start,
                fingerprint: unit.and(Fingerprint::parse(REVIEWED)),
            },
            evidence: Vec::new(),
            scores: Scores::default(),
            verdict: None,
            status: Status::Open,
            seen: 1,
            first: t,
            last: t,
            history: Vec::new(),
            model: "m".into(),
            cost_usd: 0.0,
        }
    }

    fn unit(name: &str, start: u32, end: u32, print: &str) -> Unit {
        Unit {
            name: name.into(),
            kind: UnitKind::Function,
            span: Span { start, end },
            body: None,
            fingerprint: Fingerprint::parse(print).unwrap(),
        }
    }

    #[test]
    fn a_finding_follows_its_unit_down() {
        let f = finding(Some("f"), 12, Some(10));
        let now = [unit("f", 14, 30, REVIEWED)];
        assert_eq!(
            placed(&f, &now, 40),
            Some(Placed {
                line: 16,
                stale: false
            })
        );
    }

    #[test]
    fn a_changed_unit_makes_it_stale() {
        let f = finding(Some("f"), 12, Some(10));
        let now = [unit("f", 10, 30, CHANGED)];
        assert_eq!(
            placed(&f, &now, 40),
            Some(Placed {
                line: 12,
                stale: true
            })
        );
    }

    #[test]
    fn the_line_stays_inside_a_shrunk_unit() {
        let f = finding(Some("f"), 17, Some(10));
        let now = [unit("f", 20, 22, CHANGED)];
        assert_eq!(placed(&f, &now, 40).unwrap().line, 22);
    }

    #[test]
    fn of_two_units_of_one_name_the_stored_fingerprint_wins_else_the_first() {
        let f = finding(Some("f"), 3, Some(3));
        let both = [unit("f", 1, 5, CHANGED), unit("f", 8, 12, REVIEWED)];
        assert_eq!(
            placed(&f, &both, 20),
            Some(Placed {
                line: 8,
                stale: false
            })
        );
        let neither = [unit("f", 1, 5, CHANGED), unit("f", 8, 12, "1111111")];
        assert_eq!(
            placed(&f, &neither, 20),
            Some(Placed {
                line: 1,
                stale: true
            })
        );
    }

    #[test]
    fn a_missing_unit_is_not_placed() {
        let f = finding(Some("f"), 3, Some(3));
        assert_eq!(placed(&f, &[unit("g", 1, 5, REVIEWED)], 20), None);
    }

    #[test]
    fn a_file_level_finding_keeps_its_line_and_is_never_stale() {
        let f = finding(None, 7, None);
        assert_eq!(
            placed(&f, &[unit("g", 1, 20, CHANGED)], 20),
            Some(Placed {
                line: 7,
                stale: false
            })
        );
    }

    #[test]
    fn a_file_level_finding_past_the_end_sits_on_the_last_line() {
        let f = finding(None, 30, None);
        assert_eq!(placed(&f, &[], 12).unwrap().line, 12);
        assert_eq!(placed(&f, &[], 0).unwrap().line, 1);
    }
}
