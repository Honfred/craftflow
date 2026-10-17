//! How trustworthy a number is (DESIGN.md §4.6).

use serde::{Deserialize, Serialize};

/// Any number in a pack may be measured exactly or merely estimated.
///
/// A result computed from an estimated input is itself estimated, which is
/// what [`Accuracy::combine`] is for.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Accuracy {
    #[default]
    Exact,
    Estimated,
}

impl Accuracy {
    /// The weaker of the two: anything touching an estimate is an estimate.
    #[must_use]
    pub const fn combine(self, other: Self) -> Self {
        match (self, other) {
            (Self::Exact, Self::Exact) => Self::Exact,
            _ => Self::Estimated,
        }
    }

    #[must_use]
    pub const fn is_estimated(self) -> bool {
        matches!(self, Self::Estimated)
    }
}

#[cfg(test)]
mod tests {
    use super::Accuracy;
    use super::Accuracy::{Estimated, Exact};

    #[test]
    fn combine_is_pessimistic() {
        assert_eq!(Exact.combine(Exact), Exact);
        assert_eq!(Exact.combine(Estimated), Estimated);
        assert_eq!(Estimated.combine(Exact), Estimated);
        assert_eq!(Estimated.combine(Estimated), Estimated);
    }

    #[test]
    fn defaults_to_exact() {
        assert_eq!(Accuracy::default(), Exact);
    }
}
