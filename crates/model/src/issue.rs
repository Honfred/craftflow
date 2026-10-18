//! Problems found in data that is otherwise readable.
//!
//! Lives in `model` so that `packs` and `graph` report the same shape without
//! depending on one another (DESIGN.md §3).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The data is wrong and whatever depends on it will not work.
    Error,
    /// Suspicious, but usable — a partial pack or an unfinished schema looks
    /// like this.
    Warning,
}

/// Something worth telling the user about one entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// What the issue is about, e.g. `recipe:minecraft:smelting/iron`.
    pub where_: String,
    pub message: String,
}

impl Issue {
    pub fn error(where_: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            where_: where_.into(),
            message: message.into(),
        }
    }

    pub fn warning(where_: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            where_: where_.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{label}: {}: {}", self.where_, self.message)
    }
}

/// True when nothing worse than a warning is present.
#[must_use]
pub fn is_clean(issues: &[Issue]) -> bool {
    !issues.iter().any(Issue::is_error)
}
