//! Recipes (DESIGN.md §4.4).

use serde::{Deserialize, Serialize};

use crate::accuracy::Accuracy;
use crate::id::Id;
use crate::resource::ResourceRef;
use crate::units::Ticks;

/// One side of a recipe: how much of what.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ingredient {
    pub resource: ResourceRef,
    pub amount: f64,
}

/// A recipe output, which may only appear some of the time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub resource: ResourceRef,
    pub amount: f64,
    /// Probability in `0.0..=1.0`; absent means certain.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub chance: f64,
}

const fn one() -> f64 {
    1.0
}

#[allow(clippy::trivially_copy_pass_by_ref)] // signature required by serde
fn is_one(value: &f64) -> bool {
    *value >= 1.0
}

impl Output {
    /// What the solver uses: a chance output contributes its expected value.
    #[must_use]
    pub fn expected_amount(&self) -> f64 {
        self.amount * self.chance
    }
}

/// A recipe, independent of which machine runs it.
///
/// `duration` is the recipe's own figure; the machine's `duration` formula
/// turns it into the real one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub id: Id,
    pub category: Id,
    pub duration: Ticks,
    #[serde(default)]
    pub inputs: Vec<Ingredient>,
    #[serde(default)]
    pub outputs: Vec<Output>,
    /// Needed to run, but not consumed. An item that is consumed and handed
    /// back is an ordinary input plus output instead.
    #[serde(default)]
    pub catalysts: Vec<Ingredient>,
    #[serde(default)]
    pub accuracy: Accuracy,
}

impl Recipe {
    /// A recipe with no inputs is a source: a farm, a cobble generator, a
    /// water intake (DESIGN.md §4.5).
    #[must_use]
    pub fn is_source(&self) -> bool {
        self.inputs.is_empty()
    }

    /// Every resource this recipe touches, in a stable order.
    pub fn referenced_resources(&self) -> impl Iterator<Item = &ResourceRef> {
        self.inputs
            .iter()
            .map(|i| &i.resource)
            .chain(self.catalysts.iter().map(|c| &c.resource))
            .chain(self.outputs.iter().map(|o| &o.resource))
    }
}

#[cfg(test)]
mod tests {
    use super::Recipe;
    use crate::accuracy::Accuracy;
    use crate::units::{Rate, TimeUnit, approx_eq};

    const DESIGN_EXAMPLE: &str = r##"{
        "id": "mekanism:enriching/iron_ore",
        "category": "mekanism:enriching",
        "duration": 200,
        "inputs":  [ { "resource": "#c:ores/iron", "amount": 1 } ],
        "outputs": [ { "resource": "mekanism:dust_iron", "amount": 2 },
                     { "resource": "some:byproduct", "amount": 1, "chance": 0.25 } ],
        "catalysts": [],
        "accuracy": "exact"
    }"##;

    #[test]
    fn parses_the_design_example() {
        let recipe: Recipe = serde_json::from_str(DESIGN_EXAMPLE).expect("parses");
        assert_eq!(recipe.duration.0, 200.0);
        assert_eq!(recipe.accuracy, Accuracy::Exact);
        assert!(recipe.inputs[0].resource.is_tag());
        assert!(!recipe.is_source());
    }

    #[test]
    fn chance_becomes_an_expected_amount() {
        let recipe: Recipe = serde_json::from_str(DESIGN_EXAMPLE).expect("parses");
        // No chance given: certain.
        assert!(approx_eq(recipe.outputs[0].expected_amount(), 2.0));
        // 1 at 25%.
        assert!(approx_eq(recipe.outputs[1].expected_amount(), 0.25));
    }

    #[test]
    fn output_rate_follows_from_the_duration() {
        let recipe: Recipe = serde_json::from_str(DESIGN_EXAMPLE).expect("parses");
        let rate = Rate::per_duration(recipe.outputs[0].expected_amount(), recipe.duration);
        // 2 dust every 200 ticks is 12 per minute.
        assert!(approx_eq(rate.as_per(TimeUnit::Minute), 12.0));
    }

    #[test]
    fn a_recipe_without_inputs_is_a_source() {
        let json = r#"{
            "id": "vanilla:cobble_generator",
            "category": "vanilla:generating",
            "duration": 20,
            "outputs": [ { "resource": "minecraft:cobblestone", "amount": 1 } ],
            "accuracy": "estimated"
        }"#;
        let recipe: Recipe = serde_json::from_str(json).expect("parses");
        assert!(recipe.is_source());
        assert_eq!(recipe.accuracy, Accuracy::Estimated);
    }

    #[test]
    fn referenced_resources_covers_all_three_sides() {
        let recipe: Recipe = serde_json::from_str(DESIGN_EXAMPLE).expect("parses");
        let refs: Vec<String> = recipe
            .referenced_resources()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            refs,
            ["#c:ores/iron", "mekanism:dust_iron", "some:byproduct"]
        );
    }
}
