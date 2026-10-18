//! What loaded pack data looks like to the crates that consume it.
//!
//! `solver` and `graph` need to look recipes and machines up, but DESIGN.md §3
//! puts neither of them above `packs`. The trait lives here, in the crate they
//! all share, and `packs::Registry` implements it.
//!
//! Formulas are exposed as [`Catalog::evaluate`] rather than as compiled
//! expressions on purpose: the expression type belongs to `expr`, which this
//! crate must not depend on, so evaluation stays on the `packs` side of the
//! boundary and callers only ever see numbers.

use std::collections::BTreeMap;

use thiserror::Error;

use crate::id::Id;
use crate::machine::{Machine, ParamValue};
use crate::recipe::Recipe;
use crate::resource::{Resource, Tag};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CatalogError {
    #[error("there is no machine `{machine}`")]
    NoMachine { machine: Id },

    #[error("machine `{machine}` has no formula `{formula}`")]
    NoFormula { machine: Id, formula: String },

    #[error("machine `{machine}`, formula `{formula}`: {message}")]
    Eval {
        machine: Id,
        formula: String,
        message: String,
    },
}

/// Read-only access to everything the packs defined.
pub trait Catalog {
    fn resource(&self, id: &Id) -> Option<&Resource>;
    fn tag(&self, id: &Id) -> Option<&Tag>;
    fn machine(&self, id: &Id) -> Option<&Machine>;
    fn recipe(&self, id: &Id) -> Option<&Recipe>;

    /// Evaluates one of a machine's formulas for a given recipe and parameter
    /// set. `Ok(None)` means the machine simply does not define that formula.
    fn evaluate(
        &self,
        machine: &Id,
        formula: &str,
        recipe: &Recipe,
        params: &BTreeMap<String, ParamValue>,
    ) -> Result<Option<f64>, CatalogError>;
}
