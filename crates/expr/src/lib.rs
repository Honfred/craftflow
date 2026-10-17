//! Formula engine: parsing, compilation and evaluation of pack expressions.
//!
//! See `docs/DESIGN.md` for the specification this crate implements.

// Rule 5: no `unwrap`/`expect` in library code, except provably impossible
// cases carrying a comment explaining why.
#![warn(clippy::unwrap_used, clippy::expect_used)]
