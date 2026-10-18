//! Schema graph: nodes, ports, links, networks, file format and migrations.
//!
//! See `docs/DESIGN.md` §6 and §10. The crate holds the drawing and the file
//! it is saved to; turning it into something solvable is `solver`'s job.
//!
//! Ports are implicit. A link records the resource it carries, and for a
//! machine the set of ports follows from its recipe, so switching a node's
//! recipe leaves links pointing at a resource the node no longer handles —
//! which [`validate`] reports — rather than at an identifier that no longer
//! exists.

// Rule 5: no `unwrap`/`expect` in library code, except provably impossible
// cases carrying a comment explaining why. Tests are exempt.
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod edit;
pub mod error;
pub mod migrate;
pub mod node;
pub mod schema;
pub mod validate;

pub use edit::{EditKey, Editor, HISTORY_LIMIT};
pub use error::GraphError;
pub use migrate::{CURRENT_FORMAT_VERSION, format_version, migrate};
pub use node::{
    Flow, HubNode, Link, MachineNode, Network, Node, NodeId, NodeKind, NoteNode, OutputNode,
    SourceNode,
};
pub use schema::{PackRef, Schema};
pub use validate::validate;
