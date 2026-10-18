//! Failures that stop a schema from being solved.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SolveError {
    #[error(
        "nothing in the schema limits production, so the answer is infinite: give a source a \
         rate, a machine a count, or an output a target"
    )]
    Unbounded,

    #[error("the schema has no consistent solution")]
    Infeasible,

    #[error("the LP solver failed: {0}")]
    Backend(String),
}

impl From<good_lp::ResolutionError> for SolveError {
    fn from(error: good_lp::ResolutionError) -> Self {
        match error {
            good_lp::ResolutionError::Unbounded => Self::Unbounded,
            good_lp::ResolutionError::Infeasible => Self::Infeasible,
            other => Self::Backend(other.to_string()),
        }
    }
}
