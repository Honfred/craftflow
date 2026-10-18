//! The schema file (DESIGN.md §10).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::GraphError;
use crate::migrate::{CURRENT_FORMAT_VERSION, migrate};
use crate::node::{Link, Network, Node, NodeId};

/// A pack the schema was built against, so that opening it elsewhere can say
/// what is missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRef {
    pub id: String,
    pub version: String,
}

/// A whole factory plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub format_version: u32,
    /// Which packs this was drawn against, with versions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub packs: Vec<PackRef>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub links: Vec<Link>,
    /// Named energy and stress networks.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub networks: BTreeMap<String, Network>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<model::Pattern>,
}

impl Default for Schema {
    fn default() -> Self {
        Self {
            format_version: CURRENT_FORMAT_VERSION,
            packs: Vec::new(),
            nodes: Vec::new(),
            links: Vec::new(),
            networks: BTreeMap::new(),
            patterns: Vec::new(),
        }
    }
}

impl Schema {
    /// Reads a schema, upgrading it from an older format version first.
    pub fn from_json(text: &str) -> Result<Self, GraphError> {
        let document: serde_json::Value = serde_json::from_str(text)?;
        let document = migrate(document)?;
        Ok(serde_json::from_value(document)?)
    }

    pub fn to_json_pretty(&self) -> Result<String, GraphError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    #[must_use]
    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.iter().find(|node| &node.id == id)
    }

    /// Links arriving at `id`.
    pub fn links_into<'a>(&'a self, id: &'a NodeId) -> impl Iterator<Item = &'a Link> {
        self.links.iter().filter(move |link| &link.to == id)
    }

    /// Links leaving `id`.
    pub fn links_out_of<'a>(&'a self, id: &'a NodeId) -> impl Iterator<Item = &'a Link> {
        self.links.iter().filter(move |link| &link.from == id)
    }
}
