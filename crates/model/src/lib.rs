//! Core data types: resources, tags, machines, recipes, patterns, units.
//!
//! See `docs/DESIGN.md` §4 for the specification this crate implements. The
//! crate deliberately has no dependencies of its own beyond `serde`: machine
//! formulas are kept as source text here and compiled by `packs`, which sits
//! above both this crate and `expr` in the dependency graph (§3).

// Rule 5: no `unwrap`/`expect` in library code, except provably impossible
// cases carrying a comment explaining why. Tests are exempt.
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod accuracy;
pub mod catalog;
pub mod error;
pub mod id;
pub mod issue;
pub mod machine;
pub mod pattern;
pub mod recipe;
pub mod resource;
pub mod units;

pub use accuracy::Accuracy;
pub use catalog::{Catalog, CatalogError};
pub use error::{IdError, IdPart, ParamError};
pub use id::Id;
pub use issue::{Issue, Severity};
pub use machine::{Machine, ParamSpec, ParamValue};
pub use pattern::{Pattern, PatternKind, PatternTarget, Stack};
pub use recipe::{Ingredient, Output, Recipe};
pub use resource::{Resource, ResourceKind, ResourceKindError, ResourceRef, Tag};
pub use units::{EPSILON, Rate, TICKS_PER_SECOND, Ticks, TimeUnit, approx_eq};
