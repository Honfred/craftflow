//! Machines and their tunable parameters (DESIGN.md §4.3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::accuracy::Accuracy;
use crate::error::ParamError;
use crate::id::Id;

/// What a machine parameter accepts: the knobs shown in the inspector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParamSpec {
    Int {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<i64>,
        default: i64,
    },
    Float {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<f64>,
        default: f64,
    },
    Bool {
        default: bool,
    },
}

impl ParamSpec {
    /// The value this parameter starts at.
    #[must_use]
    pub const fn default_value(&self) -> ParamValue {
        match *self {
            Self::Int { default, .. } => ParamValue::Int(default),
            Self::Float { default, .. } => ParamValue::Float(default),
            Self::Bool { default } => ParamValue::Bool(default),
        }
    }

    /// Checks a user-supplied value against this specification.
    ///
    /// `name` only ever appears in the error message.
    pub fn validate(&self, name: &str, value: &ParamValue) -> Result<(), ParamError> {
        let mismatch = |expected: &'static str| ParamError::Type {
            name: name.to_owned(),
            expected,
            got: value.type_name(),
        };

        match (self, value) {
            (Self::Int { min, max, .. }, &ParamValue::Int(got)) => check_range(
                name,
                got as f64,
                min.map(|m| m as f64),
                max.map(|m| m as f64),
            ),
            (Self::Float { min, max, .. }, &ParamValue::Float(got)) => {
                check_range(name, got, *min, *max)
            }
            (Self::Bool { .. }, ParamValue::Bool(_)) => Ok(()),
            (Self::Int { .. }, _) => Err(mismatch("an integer")),
            (Self::Float { .. }, _) => Err(mismatch("a number")),
            (Self::Bool { .. }, _) => Err(mismatch("a boolean")),
        }
    }
}

fn check_range(name: &str, got: f64, min: Option<f64>, max: Option<f64>) -> Result<(), ParamError> {
    if let Some(min) = min
        && got < min
    {
        return Err(ParamError::BelowMin {
            name: name.to_owned(),
            min,
            got,
        });
    }
    if let Some(max) = max
        && got > max
    {
        return Err(ParamError::AboveMax {
            name: name.to_owned(),
            max,
            got,
        });
    }
    Ok(())
}

/// A concrete value for a machine parameter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    // `Bool` comes first: untagged deserialisation tries the variants in
    // order, and `true` must not be read as a number.
    Bool(bool),
    Int(i64),
    Float(f64),
}

impl ParamValue {
    /// Parameters feed the formula engine, which works entirely in `f64`.
    #[must_use]
    pub fn as_f64(self) -> f64 {
        match self {
            Self::Bool(b) => f64::from(u8::from(b)),
            Self::Int(i) => i as f64,
            Self::Float(f) => f,
        }
    }

    #[must_use]
    pub const fn type_name(self) -> &'static str {
        match self {
            Self::Bool(_) => "a boolean",
            Self::Int(_) => "an integer",
            Self::Float(_) => "a number",
        }
    }
}

/// A machine: what it can run, what the user can tune, and how that changes
/// its speed and draw.
///
/// `formulas` holds expression **source**, not compiled trees: this crate sits
/// below `expr` in the dependency graph (DESIGN.md §3), so compiling them is
/// the job of `packs`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub id: Id,
    /// Recipe categories this machine can run.
    #[serde(default)]
    pub categories: Vec<Id>,
    #[serde(default)]
    pub params: BTreeMap<String, ParamSpec>,
    #[serde(default)]
    pub formulas: BTreeMap<String, String>,
    #[serde(default)]
    pub constants: BTreeMap<String, f64>,
    /// Named networks this machine attaches to, e.g. `energy` →
    /// `mekanism:joules`.
    #[serde(default)]
    pub networks: BTreeMap<String, Id>,
    #[serde(default)]
    pub accuracy: Accuracy,
}

impl Machine {
    /// Every parameter at its default, ready to hand to the formula engine.
    #[must_use]
    pub fn default_params(&self) -> BTreeMap<String, ParamValue> {
        self.params
            .iter()
            .map(|(name, spec)| (name.clone(), spec.default_value()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Machine, ParamSpec, ParamValue};
    use crate::error::ParamError;

    #[test]
    fn parses_the_design_example() {
        let json = r#"{
            "id": "mekanism:enrichment_chamber",
            "categories": ["mekanism:enriching"],
            "params": {
                "speed_upgrades":  { "type": "int", "min": 0, "max": 8, "default": 0 },
                "energy_upgrades": { "type": "int", "min": 0, "max": 8, "default": 0 }
            },
            "formulas": {
                "duration": "recipe.duration * speed_factor(speed_upgrades)",
                "energy_per_tick": "base_energy * energy_factor(speed_upgrades, energy_upgrades)"
            },
            "constants": { "base_energy": 50 },
            "networks": { "energy": "mekanism:joules" },
            "accuracy": "exact"
        }"#;

        let machine: Machine = serde_json::from_str(json).expect("parses");
        assert_eq!(machine.params.len(), 2);
        assert_eq!(machine.constants["base_energy"], 50.0);
        assert_eq!(machine.networks["energy"].to_string(), "mekanism:joules");
        assert_eq!(
            machine.default_params()["speed_upgrades"],
            ParamValue::Int(0)
        );
    }

    #[test]
    fn validate_enforces_bounds() {
        let spec = ParamSpec::Int {
            min: Some(0),
            max: Some(8),
            default: 0,
        };
        assert!(spec.validate("speed_upgrades", &ParamValue::Int(4)).is_ok());
        assert!(matches!(
            spec.validate("speed_upgrades", &ParamValue::Int(9)),
            Err(ParamError::AboveMax { .. })
        ));
        assert!(matches!(
            spec.validate("speed_upgrades", &ParamValue::Int(-1)),
            Err(ParamError::BelowMin { .. })
        ));
    }

    #[test]
    fn validate_rejects_the_wrong_type() {
        let spec = ParamSpec::Bool { default: false };
        assert!(matches!(
            spec.validate("enabled", &ParamValue::Int(1)),
            Err(ParamError::Type { .. })
        ));
    }

    #[test]
    fn values_reach_the_formula_engine_as_f64() {
        assert_eq!(ParamValue::Bool(true).as_f64(), 1.0);
        assert_eq!(ParamValue::Bool(false).as_f64(), 0.0);
        assert_eq!(ParamValue::Int(7).as_f64(), 7.0);
    }

    #[test]
    fn untagged_values_keep_their_type() {
        assert_eq!(
            serde_json::from_str::<ParamValue>("true").expect("parses"),
            ParamValue::Bool(true)
        );
        assert_eq!(
            serde_json::from_str::<ParamValue>("3").expect("parses"),
            ParamValue::Int(3)
        );
        assert_eq!(
            serde_json::from_str::<ParamValue>("3.5").expect("parses"),
            ParamValue::Float(3.5)
        );
    }
}
