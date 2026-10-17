//! Error types produced while parsing and validating model values.

use std::fmt;

use thiserror::Error;

/// Which half of a `namespace:path` identifier a problem belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdPart {
    Namespace,
    Path,
}

impl IdPart {
    /// The characters this part is allowed to contain, for error messages.
    const fn allowed(self) -> &'static str {
        match self {
            Self::Namespace => "a-z 0-9 _ - .",
            Self::Path => "a-z 0-9 _ - . /",
        }
    }
}

impl fmt::Display for IdPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Namespace => f.write_str("namespace"),
            Self::Path => f.write_str("path"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IdError {
    #[error("`{input}` is not a valid id: expected `namespace:path`")]
    Shape { input: String },
    #[error("`{input}` is not a valid id: the {part} is empty")]
    Empty { input: String, part: IdPart },
    #[error(
        "`{input}` is not a valid id: the {part} contains `{ch}`, allowed characters are {allowed}"
    )]
    Char {
        input: String,
        part: IdPart,
        ch: char,
        allowed: &'static str,
    },
}

impl IdError {
    pub(crate) fn char(input: &str, part: IdPart, ch: char) -> Self {
        Self::Char {
            input: input.to_owned(),
            part,
            ch,
            allowed: part.allowed(),
        }
    }
}

/// A machine parameter was given a value its specification does not accept.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum ParamError {
    #[error("parameter `{name}` expects {expected}, got {got}")]
    Type {
        name: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("parameter `{name}` must be at least {min}, got {got}")]
    BelowMin { name: String, min: f64, got: f64 },
    #[error("parameter `{name}` must be at most {max}, got {got}")]
    AboveMax { name: String, max: f64, got: f64 },
}
