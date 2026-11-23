//! The broken region: where the parser lost the thread. When a line is in no unit because the
//! file does not parse there (a stray `{` swallowed the function), the lines the parser marked
//! as an error still say where a person would look.

use crate::Result;
use crate::domain::fingerprint::fingerprint;
use crate::domain::language::Loaded;
use crate::domain::unit::{Span, Unit, UnitKind, lines_of, unit_at};

/// The name a broken region has, where a unit's name would go.
pub const BROKEN_REGION_NAME: &str = "broken region";

/// The unit holding the line; when the line is in no unit, the smallest stretch the parser
/// marked as an error around it, as a unit of kind [`UnitKind::BrokenRegion`]. `None` when
/// neither holds the line.
///
/// Example: for `fn handle() {\n    if the party is too big say so\n    let n = 1;\n}\n` the
/// parser loses `handle`, so line 2 is in no unit, but the error region spans lines 1 to 4 and
/// comes back; for a file that parses, this is `unit_at` and nothing else.
pub fn unit_or_broken_region_at(
    lang: &Loaded,
    text: &str,
    units: &[Unit],
    line: u32,
) -> Result<Option<Unit>> {
    if let Some(unit) = unit_at(units, line) {
        return Ok(Some(unit.clone()));
    }
    broken_region_at(lang, text, line)
}

/// The smallest parser error region that holds the line, `None` when the file parses there.
///
/// Example: with an unclosed `if` on line 2 of a function, the region holds line 2.
pub fn broken_region_at(lang: &Loaded, text: &str, line: u32) -> Result<Option<Unit>> {
    let Some(tree) = lang.parse(text)? else {
        return Ok(None);
    };
    let mut innermost: Option<Span> = None;
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        let lines = lines_of(&node);
        if !lines.holds(line) {
            continue;
        }
        if node.is_error() && innermost.is_none_or(|best| size(lines) <= size(best)) {
            innermost = Some(lines);
        }
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    Ok(innermost.map(|span| broken_region(text, span)))
}

fn broken_region(text: &str, span: Span) -> Unit {
    Unit {
        name: BROKEN_REGION_NAME.to_string(),
        kind: UnitKind::BrokenRegion,
        span,
        body: None,
        fingerprint: fingerprint(text, span),
    }
}

fn size(span: Span) -> u32 {
    span.end - span.start
}
