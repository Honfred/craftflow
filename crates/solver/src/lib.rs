//! Reduction of a schema to processes and the LP that solves them.
//!
//! See `docs/DESIGN.md` §6.3 and §6.4. Every node becomes zero or more
//! [`Process`]es and the solver sees nothing else (rule 3); all constraints
//! are linear.
//!
//! The backend is `good_lp`. `microlp` is the default because it is pure Rust
//! and keeps CI simple on all three platforms; building with the `highs`
//! feature swaps in HiGHS, which is faster on large problems but needs a C++
//! toolchain.

// Rule 5: no `unwrap`/`expect` in library code, except provably impossible
// cases carrying a comment explaining why. Tests are exempt.
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod error;
pub mod lp;
pub mod process;
pub mod reduce;
pub mod report;

pub use error::SolveError;
pub use lp::{NETWORK_FORMULA_SUFFIX, solve};
pub use process::{Process, ProcessRole, Term};
pub use reduce::{DURATION_FORMULA, Reduced, reduce};
pub use report::{LinkFlow, MachineLoad, NetworkBalance, OutputResult, Report, Surplus};
