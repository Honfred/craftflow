//! Reduction of a schema to processes and the LP that solves them.
//!
//! See `docs/DESIGN.md` for the specification this crate implements.

// Rule 5: no `unwrap`/`expect` in library code, except provably impossible
// cases carrying a comment explaining why.
#![warn(clippy::unwrap_used, clippy::expect_used)]
