//! Errors, source positions, and the rendering that turns them into something
//! a pack author can act on.

use std::fmt;

use thiserror::Error;

use crate::env::Arity;

/// A byte range inside the formula source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// 1-based line and column of the span's start, counted in characters.
    #[must_use]
    pub fn line_column(self, source: &str) -> (usize, usize) {
        let mut line = 1;
        let mut column = 1;
        for (offset, ch) in source.char_indices() {
            if offset >= self.start {
                break;
            }
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        (line, column)
    }
}

/// A formula could not be turned into something evaluable.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum CompileError {
    #[error("unexpected character `{ch}`")]
    UnexpectedChar { span: Span, ch: char },

    #[error("unexpected `{found}`, expected {expected}")]
    UnexpectedToken {
        span: Span,
        found: String,
        expected: &'static str,
    },

    #[error("the formula ends early, expected {expected}")]
    UnexpectedEnd { span: Span, expected: &'static str },

    #[error("`{text}` is not a valid number")]
    InvalidNumber { span: Span, text: String },

    #[error("unknown function `{name}`{}", suggestion_hint(.suggestion))]
    UnknownFunction {
        span: Span,
        name: String,
        suggestion: Option<String>,
    },

    #[error("unknown variable `{name}`{}", suggestion_hint(.suggestion))]
    UnknownVariable {
        span: Span,
        name: String,
        suggestion: Option<String>,
    },

    #[error("`{name}` takes {expected}, but got {got}")]
    WrongArity {
        span: Span,
        name: String,
        expected: Arity,
        got: usize,
    },

    #[error("`{name}` is already defined")]
    DuplicateFunction { span: Span, name: String },

    #[error("the formula is empty")]
    Empty { span: Span },
}

fn suggestion_hint(suggestion: &Option<String>) -> String {
    match suggestion {
        Some(name) => format!(", did you mean `{name}`?"),
        None => String::new(),
    }
}

impl CompileError {
    #[must_use]
    pub const fn span(&self) -> Span {
        match self {
            Self::UnexpectedChar { span, .. }
            | Self::UnexpectedToken { span, .. }
            | Self::UnexpectedEnd { span, .. }
            | Self::InvalidNumber { span, .. }
            | Self::UnknownFunction { span, .. }
            | Self::UnknownVariable { span, .. }
            | Self::WrongArity { span, .. }
            | Self::DuplicateFunction { span, .. }
            | Self::Empty { span } => *span,
        }
    }

    /// A multi-line report pointing at the offending part of the formula:
    ///
    /// ```text
    /// recipe.duration * speed_factr(n)
    ///                   ^^^^^^^^^^^
    /// line 1, column 19: unknown function `speed_factr`, did you mean `speed_factor`?
    /// ```
    #[must_use]
    pub fn report(&self, source: &str) -> String {
        let span = self.span();
        let (line_no, column) = span.line_column(source);

        // The line the span starts on, without its trailing newline.
        let line = source
            .lines()
            .nth(line_no.saturating_sub(1))
            .unwrap_or(source);

        // Spans can run past the end of the line (an unexpected end of input),
        // so clamp the underline to what is actually there.
        let width = source
            .get(span.start..span.end)
            .map_or(1, |text| text.chars().count())
            .max(1);
        let pad = " ".repeat(column.saturating_sub(1));
        let caret = "^".repeat(width);

        format!("{line}\n{pad}{caret}\nline {line_no}, column {column}: {self}")
    }
}

/// A compiled formula could not be evaluated.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum EvalError {
    #[error("no value for variable `{name}`")]
    UnknownVariable { name: String },

    #[error("division by zero")]
    DivisionByZero,

    #[error("formula nested more than {limit} calls deep, is a function calling itself?")]
    TooDeep { limit: usize },

    #[error("function `{function}` failed: {message}")]
    Native { function: String, message: String },
}

/// Levenshtein distance, used only to suggest a name the author probably meant.
fn distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b_chars.len()).collect();
    let mut current = vec![0; b_chars.len() + 1];

    for (i, a_ch) in a.chars().enumerate() {
        current[0] = i + 1;
        for (j, &b_ch) in b_chars.iter().enumerate() {
            let cost = usize::from(a_ch != b_ch);
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b_chars.len()]
}

/// The closest known name to `name`, if one is close enough to be worth
/// mentioning.
pub(crate) fn closest<'a>(name: &str, known: impl Iterator<Item = &'a str>) -> Option<String> {
    // A third of the length, so short names need a near-exact match and long
    // ones tolerate a typo or two.
    let limit = (name.chars().count() / 3).max(1);
    known
        .map(|candidate| (distance(name, candidate), candidate))
        .filter(|&(d, _)| d <= limit)
        .min_by_key(|&(d, candidate)| (d, candidate.len()))
        .map(|(_, candidate)| candidate.to_owned())
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::{Span, closest, distance};

    #[test]
    fn line_and_column_are_one_based() {
        let source = "a +\n  bad";
        assert_eq!(Span::new(0, 1).line_column(source), (1, 1));
        assert_eq!(Span::new(6, 9).line_column(source), (2, 3));
    }

    #[test]
    fn distance_counts_edits() {
        assert_eq!(distance("speed_factor", "speed_factor"), 0);
        assert_eq!(distance("speed_factr", "speed_factor"), 1);
        assert_eq!(distance("", "abc"), 3);
    }

    #[test]
    fn closest_only_suggests_near_misses() {
        let known = ["speed_factor", "energy_factor"];
        assert_eq!(
            closest("speed_factr", known.iter().copied()),
            Some("speed_factor".to_owned())
        );
        assert_eq!(closest("totally_different", known.iter().copied()), None);
    }
}
