//! What a solved schema looks like (DESIGN.md §6.4).

use std::collections::BTreeMap;

use graph::NodeId;
use model::{Accuracy, Id, Issue, Rate};

/// Flow along one link.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkFlow {
    /// Index into the schema's `links`.
    pub index: usize,
    pub from: NodeId,
    pub to: NodeId,
    pub resource: Id,
    pub rate: Rate,
    /// Set when the link ran into its own capacity.
    pub at_capacity: bool,
}

/// How hard a machine node is working.
#[derive(Debug, Clone, PartialEq)]
pub struct MachineLoad {
    pub node: NodeId,
    pub machine: Id,
    pub recipe: Id,
    /// Activations per tick.
    pub activity: f64,
    /// Machines actually running, which may be fractional.
    pub running: f64,
    /// Machines placed, if the schema says.
    pub count: Option<f64>,
    /// `running / count`, absent when the node has no count.
    pub load: Option<f64>,
}

impl MachineLoad {
    /// The schema cannot go faster here.
    #[must_use]
    pub fn is_bottleneck(&self) -> bool {
        self.load
            .is_some_and(|load| load >= 1.0 - f64::EPSILON.sqrt())
    }
}

/// What an output node actually got.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputResult {
    pub node: NodeId,
    pub resource: Id,
    pub achieved: Rate,
    pub target: Option<Rate>,
}

impl OutputResult {
    /// How far short of the target this came, if there was one.
    #[must_use]
    pub fn shortfall(&self) -> Option<Rate> {
        self.target.map(|target| {
            Rate::per_tick((target.as_per_tick() - self.achieved.as_per_tick()).max(0.0))
        })
    }
}

/// Material that arrived somewhere and was not used.
#[derive(Debug, Clone, PartialEq)]
pub struct Surplus {
    pub node: NodeId,
    pub resource: Id,
    pub rate: Rate,
}

/// Generation against consumption on one named network (DESIGN.md §6.2).
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkBalance {
    pub name: String,
    pub kind: Id,
    pub produced: Rate,
    pub consumed: Rate,
}

impl NetworkBalance {
    /// Positive means spare capacity, negative means the network is short.
    #[must_use]
    pub fn net(&self) -> Rate {
        Rate::per_tick(self.produced.as_per_tick() - self.consumed.as_per_tick())
    }

    #[must_use]
    pub fn is_short(&self) -> bool {
        self.net().as_per_tick() < -f64::EPSILON.sqrt()
    }
}

/// Everything the UI and the CLI show after a solve.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub links: Vec<LinkFlow>,
    pub machines: Vec<MachineLoad>,
    pub outputs: Vec<OutputResult>,
    pub sources: Vec<OutputResult>,
    pub surplus: Vec<Surplus>,
    pub networks: Vec<NetworkBalance>,
    /// Items a recipe needs present but does not consume (DESIGN.md §4.4).
    pub catalysts: BTreeMap<Id, f64>,
    /// `Estimated` when any number that went into this was itself a guess.
    pub accuracy: Accuracy,
    pub issues: Vec<Issue>,
}

impl Report {
    #[must_use]
    pub fn bottlenecks(&self) -> Vec<&MachineLoad> {
        self.machines
            .iter()
            .filter(|machine| machine.is_bottleneck())
            .collect()
    }
}
