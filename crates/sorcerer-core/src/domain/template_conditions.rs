//! The condition of an `{= if … =}` or `{= elif … =}`: read from its text, then decided against
//! the values. `not` binds tightest, then `and`, then `or`; there are no parentheses.

use crate::domain::template_paths::{Datum, Lookup, Path, parse_path};

/// A condition as written.
///
/// Example: `not a and b or c` is `Or(And(Not(Truth(a)), Truth(b)), Truth(c))`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition<'t> {
    /// A value alone: whether it holds (see [`Datum::holds`]).
    Truth(Operand<'t>),
    Equal(Operand<'t>, Operand<'t>),
    NotEqual(Operand<'t>, Operand<'t>),
    Not(Box<Condition<'t>>),
    And(Box<Condition<'t>>, Box<Condition<'t>>),
    Or(Box<Condition<'t>>, Box<Condition<'t>>),
}

/// One side of a comparison, or the whole of a truth test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operand<'t> {
    Path(Path<'t>),
    /// `"text"`, without escapes.
    Quoted(&'t str),
}

/// Reads a condition, the tag's text (`if x =}` is quoted as `` `{= if x =}` ``) and line given
/// for the errors.
///
/// Example: `parse_condition("model == \"dish\"", "if model == \"dish\"", 1)` is
/// `Equal(Path(model), Quoted("dish"))`; `parse_condition("(a)", "if (a)", 1)` is an error
/// saying parentheses are not supported.
pub fn parse_condition<'t>(text: &'t str, tag: &str, line: usize) -> Result<Condition<'t>, String> {
    let fail = |why: String| format!("`{{= {tag} =}}` (line {line}): {why}");
    let tokens = tokens_of(text).map_err(fail)?;
    let mut reader = Reader {
        tokens: &tokens,
        at: 0,
        line,
    };
    let condition = reader.or().map_err(fail)?;
    match reader.tokens.get(reader.at) {
        None => Ok(condition),
        Some(extra) => Err(fail(format!("`{}` is not expected here", extra.written()))),
    }
}

/// Whether the condition holds for these values.
pub(crate) fn decide(
    condition: &Condition<'_>,
    lookup: &Lookup<'_>,
    template: &str,
) -> Result<bool, String> {
    match condition {
        Condition::Truth(operand) => Ok(value_of(operand, lookup, template)?.holds()),
        Condition::Equal(left, right) => compare(left, right, lookup, template),
        Condition::NotEqual(left, right) => Ok(!compare(left, right, lookup, template)?),
        Condition::Not(inner) => Ok(!decide(inner, lookup, template)?),
        Condition::And(left, right) => {
            Ok(decide(left, lookup, template)? && decide(right, lookup, template)?)
        }
        Condition::Or(left, right) => {
            Ok(decide(left, lookup, template)? || decide(right, lookup, template)?)
        }
    }
}

/// The paths a condition reads, left to right.
pub(crate) fn paths_of<'c, 't>(condition: &'c Condition<'t>) -> Vec<&'c Path<'t>> {
    let of_operand = |operand: &'c Operand<'t>| match operand {
        Operand::Path(path) => vec![path],
        Operand::Quoted(_) => vec![],
    };
    match condition {
        Condition::Truth(operand) => of_operand(operand),
        Condition::Equal(left, right) | Condition::NotEqual(left, right) => {
            [of_operand(left), of_operand(right)].concat()
        }
        Condition::Not(inner) => paths_of(inner),
        Condition::And(left, right) | Condition::Or(left, right) => {
            [paths_of(left), paths_of(right)].concat()
        }
    }
}

fn compare(
    left: &Operand<'_>,
    right: &Operand<'_>,
    lookup: &Lookup<'_>,
    template: &str,
) -> Result<bool, String> {
    Ok(text_of(left, lookup, template)? == text_of(right, lookup, template)?)
}

/// An operand's text, for a comparison: a flag or a list cannot be compared.
fn text_of(operand: &Operand<'_>, lookup: &Lookup<'_>, template: &str) -> Result<String, String> {
    match (operand, value_of(operand, lookup, template)?) {
        (_, Datum::Text(text)) => Ok(text),
        (Operand::Path(path), Datum::Flag(_)) => Err(format!(
            "`{path}` (line {}) is a flag: test it with `if` or `not`",
            path.line
        )),
        (Operand::Path(path), other) => Err(format!(
            "`{path}` (line {}) is {}: only text compares with `==` or `!=`",
            path.line,
            other.sort()
        )),
        (Operand::Quoted(_), _) => unreachable!("a quoted operand is text"),
    }
}

fn value_of(operand: &Operand<'_>, lookup: &Lookup<'_>, template: &str) -> Result<Datum, String> {
    match operand {
        Operand::Path(path) => lookup.resolve(path, template).cloned(),
        Operand::Quoted(text) => Ok(Datum::Text((*text).to_string())),
    }
}

/// One word of a condition.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token<'t> {
    Word(&'t str),
    Quoted(&'t str),
    Equal,
    NotEqual,
}

impl Token<'_> {
    fn written(&self) -> String {
        match self {
            Token::Word(word) => (*word).to_string(),
            Token::Quoted(text) => format!("\"{text}\""),
            Token::Equal => "==".into(),
            Token::NotEqual => "!=".into(),
        }
    }
}

/// The condition's words: names and paths, quoted texts, `==` and `!=`.
fn tokens_of(text: &str) -> Result<Vec<Token<'_>>, String> {
    let mut tokens = Vec::new();
    let mut rest = text.trim_start();
    while let Some(first) = rest.chars().next() {
        let (token, after) = match first {
            '(' | ')' => {
                return Err("parentheses are not supported; split the condition with elif".into());
            }
            '"' => {
                let close = rest[1..]
                    .find('"')
                    .ok_or_else(|| format!("the quote `{rest}` is never closed"))?;
                (Token::Quoted(&rest[1..=close]), &rest[close + 2..])
            }
            '=' | '!' => match rest.get(..2) {
                Some("==") => (Token::Equal, &rest[2..]),
                Some("!=") => (Token::NotEqual, &rest[2..]),
                _ => {
                    return Err(format!(
                        "`{first}` alone means nothing; compare with == or !="
                    ));
                }
            },
            _ => {
                let end = rest
                    .find(|c: char| c.is_whitespace() || "\"=!()".contains(c))
                    .unwrap_or(rest.len());
                (Token::Word(&rest[..end]), &rest[end..])
            }
        };
        tokens.push(token);
        rest = after.trim_start();
    }
    Ok(tokens)
}

/// Reads tokens by the grammar, one rule per method.
struct Reader<'r, 't> {
    tokens: &'r [Token<'t>],
    at: usize,
    line: usize,
}

impl<'t> Reader<'_, 't> {
    fn or(&mut self) -> Result<Condition<'t>, String> {
        let mut condition = self.and()?;
        while self.take_word("or") {
            condition = Condition::Or(Box::new(condition), Box::new(self.and()?));
        }
        Ok(condition)
    }

    fn and(&mut self) -> Result<Condition<'t>, String> {
        let mut condition = self.not()?;
        while self.take_word("and") {
            condition = Condition::And(Box::new(condition), Box::new(self.not()?));
        }
        Ok(condition)
    }

    fn not(&mut self) -> Result<Condition<'t>, String> {
        if self.take_word("not") {
            Ok(Condition::Not(Box::new(self.not()?)))
        } else {
            self.comparison()
        }
    }

    fn comparison(&mut self) -> Result<Condition<'t>, String> {
        let left = self.operand()?;
        match self.tokens.get(self.at) {
            Some(Token::Equal) => {
                self.at += 1;
                Ok(Condition::Equal(left, self.operand()?))
            }
            Some(Token::NotEqual) => {
                self.at += 1;
                Ok(Condition::NotEqual(left, self.operand()?))
            }
            _ => Ok(Condition::Truth(left)),
        }
    }

    fn operand(&mut self) -> Result<Operand<'t>, String> {
        let token = self
            .tokens
            .get(self.at)
            .ok_or("the condition ends too soon")?;
        self.at += 1;
        match token {
            Token::Quoted(text) => Ok(Operand::Quoted(text)),
            Token::Word(word) if !["and", "or", "not"].contains(word) => {
                parse_path(word, self.line)
                    .map(Operand::Path)
                    .ok_or_else(|| format!("`{word}` is not a name or a path"))
            }
            other => Err(format!("`{}` is not expected here", other.written())),
        }
    }

    fn take_word(&mut self, word: &str) -> bool {
        let found = self.tokens.get(self.at) == Some(&Token::Word(word));
        if found {
            self.at += 1;
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn truth(name: &str) -> Condition<'_> {
        Condition::Truth(Operand::Path(parse_path(name, 1).unwrap()))
    }

    #[test]
    fn not_binds_tighter_than_and_than_or() {
        let read = parse_condition("not a and b or c", "if …", 1).unwrap();
        let expected = Condition::Or(
            Box::new(Condition::And(
                Box::new(Condition::Not(Box::new(truth("a")))),
                Box::new(truth("b")),
            )),
            Box::new(truth("c")),
        );
        assert_eq!(read, expected);
    }

    #[test]
    fn a_comparison_reads_both_sides() {
        let read = parse_condition("model != \"dish\"", "if …", 1).unwrap();
        assert!(matches!(
            read,
            Condition::NotEqual(_, Operand::Quoted("dish"))
        ));
    }

    #[test]
    fn leftover_words_and_lone_marks_are_errors() {
        let extra = parse_condition("a b", "if a b", 2).unwrap_err();
        assert_eq!(extra, "`{= if a b =}` (line 2): `b` is not expected here");
        assert!(parse_condition("a = b", "if a = b", 1).is_err());
        assert!(parse_condition("", "if", 1).is_err());
        assert!(parse_condition("a ==", "if a ==", 1).is_err());
    }
}
