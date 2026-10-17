//! Patterns (DESIGN.md §4.7).

use serde::{Deserialize, Serialize};

use crate::id::Id;

/// How a pattern is executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternKind {
    /// Run by an assembler inside the ME node itself.
    Crafting,
    /// Bound to a node on the schema, which does the work.
    Processing,
}

/// A fixed quantity of one resource. Patterns never refer to tags: the network
/// sends specific items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    pub resource: Id,
    pub amount: f64,
}

/// The node a processing pattern is bound to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatternTarget {
    pub node: String,
}

/// What the network sends out and what it waits for in return.
///
/// Deliberately separate from [`crate::recipe::Recipe`]: a pattern describes an
/// agreement with the network, and the solver's job is to check it against
/// what the target node really does (DESIGN.md §4.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: Id,
    pub kind: PatternKind,
    #[serde(default)]
    pub sends: Vec<Stack>,
    #[serde(default)]
    pub expects: Vec<Stack>,
    /// Required for `processing`, absent for `crafting`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<PatternTarget>,
}

impl Pattern {
    /// Resources that come back unchanged, such as a smithing template.
    ///
    /// These are what the §11 scenario 5 validator cares about: if the target
    /// node does not hand them back, the pattern deadlocks.
    pub fn round_trip_resources(&self) -> impl Iterator<Item = &Id> {
        self.sends.iter().map(|s| &s.resource).filter(|sent| {
            self.expects
                .iter()
                .any(|expected| &&expected.resource == sent)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Pattern, PatternKind};

    const NETHERITE: &str = r#"{
        "id": "user:netherite_sword_upgrade",
        "kind": "processing",
        "sends":   [ { "resource": "minecraft:diamond_sword", "amount": 1 },
                     { "resource": "minecraft:netherite_ingot", "amount": 1 },
                     { "resource": "minecraft:netherite_upgrade_smithing_template", "amount": 1 } ],
        "expects": [ { "resource": "minecraft:netherite_sword", "amount": 1 },
                     { "resource": "minecraft:netherite_upgrade_smithing_template", "amount": 1 } ],
        "target": { "node": "node-7" }
    }"#;

    #[test]
    fn parses_the_design_example() {
        let pattern: Pattern = serde_json::from_str(NETHERITE).expect("parses");
        assert_eq!(pattern.kind, PatternKind::Processing);
        assert_eq!(pattern.sends.len(), 3);
        assert_eq!(pattern.expects.len(), 2);
        assert_eq!(
            pattern.target.as_ref().map(|t| t.node.as_str()),
            Some("node-7")
        );
    }

    #[test]
    fn finds_the_smithing_template_coming_back() {
        let pattern: Pattern = serde_json::from_str(NETHERITE).expect("parses");
        let round_trip: Vec<String> = pattern
            .round_trip_resources()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            round_trip,
            ["minecraft:netherite_upgrade_smithing_template"]
        );
    }

    #[test]
    fn a_crafting_pattern_needs_no_target() {
        let json = r#"{
            "id": "user:planks",
            "kind": "crafting",
            "sends": [ { "resource": "minecraft:oak_log", "amount": 1 } ],
            "expects": [ { "resource": "minecraft:oak_planks", "amount": 4 } ]
        }"#;
        let pattern: Pattern = serde_json::from_str(json).expect("parses");
        assert_eq!(pattern.kind, PatternKind::Crafting);
        assert!(pattern.target.is_none());
        assert_eq!(pattern.round_trip_resources().count(), 0);
    }
}
