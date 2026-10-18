//! The solver's only primitive (DESIGN.md §6.3, rule 3).
//!
//! Every node becomes zero or more processes, and the solver knows nothing
//! else. A plugin node will return processes from Luau the same way, which is
//! why the type is public and plain.

use graph::NodeId;
use model::{Accuracy, ResourceRef};

/// How much of what, per one activation.
#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub resource: ResourceRef,
    pub amount: f64,
}

impl Term {
    pub fn new(resource: ResourceRef, amount: f64) -> Self {
        Self { resource, amount }
    }
}

/// Which node shape a process came from, which decides how it is read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessRole {
    /// A machine running a recipe.
    Machine,
    /// Material entering the schema.
    Source,
    /// Material leaving it; these are what the objective maximises.
    Output,
}

/// One linear activity the solver may run at some rate.
///
/// "Activity" is activations per tick. A machine taking 200 ticks per craft
/// runs at 0.005 activations per tick, and eight of them at 0.04.
#[derive(Debug, Clone, PartialEq)]
pub struct Process {
    pub node: NodeId,
    pub role: ProcessRole,
    pub inputs: Vec<Term>,
    pub outputs: Vec<Term>,
    pub min_activity: f64,
    /// `None` means the solver may run this as fast as the rest of the schema
    /// allows.
    pub max_activity: Option<f64>,
    /// Ticks per activation, kept so activity can be turned back into a
    /// number of running machines for the network draw.
    pub duration: Option<f64>,
    /// Weight for the calculator's cost objective (DESIGN.md §7). The analysis
    /// mode of §6.4 does not read it.
    pub cost: f64,
    pub accuracy: Accuracy,
}

impl Process {
    pub fn new(node: NodeId, role: ProcessRole) -> Self {
        Self {
            node,
            role,
            inputs: Vec::new(),
            outputs: Vec::new(),
            min_activity: 0.0,
            max_activity: None,
            duration: None,
            cost: 0.0,
            accuracy: Accuracy::Exact,
        }
    }

    /// How many machines are running when this process runs at `activity`.
    #[must_use]
    pub fn running_machines(&self, activity: f64) -> Option<f64> {
        self.duration.map(|duration| activity * duration)
    }
}
