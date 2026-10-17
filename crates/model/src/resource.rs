//! Resources and tags (DESIGN.md §4.1, §4.2).

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::IdError;
use crate::id::Id;

/// What kind of thing flows along a link.
///
/// The storage unit follows from the kind: items count in pieces, fluids in
/// mB, stress in SU, and energy in whatever [`Resource::unit`] names.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ResourceKind {
    Item,
    Fluid,
    Energy,
    Stress,
    /// `custom:<plugin>/<name>`, registered by a plugin.
    Custom {
        plugin: String,
        name: String,
    },
}

impl fmt::Display for ResourceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Item => f.write_str("item"),
            Self::Fluid => f.write_str("fluid"),
            Self::Energy => f.write_str("energy"),
            Self::Stress => f.write_str("stress"),
            Self::Custom { plugin, name } => write!(f, "custom:{plugin}/{name}"),
        }
    }
}

impl FromStr for ResourceKind {
    type Err = ResourceKindError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "item" => Ok(Self::Item),
            "fluid" => Ok(Self::Fluid),
            "energy" => Ok(Self::Energy),
            "stress" => Ok(Self::Stress),
            other => {
                let rest =
                    other
                        .strip_prefix("custom:")
                        .ok_or_else(|| ResourceKindError::Unknown {
                            input: other.to_owned(),
                        })?;
                let (plugin, name) =
                    rest.split_once('/')
                        .ok_or_else(|| ResourceKindError::CustomShape {
                            input: other.to_owned(),
                        })?;
                if plugin.is_empty() || name.is_empty() {
                    return Err(ResourceKindError::CustomShape {
                        input: other.to_owned(),
                    });
                }
                Ok(Self::Custom {
                    plugin: plugin.to_owned(),
                    name: name.to_owned(),
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResourceKindError {
    #[error(
        "unknown resource kind `{input}`, expected item, fluid, energy, stress or custom:<plugin>/<name>"
    )]
    Unknown { input: String },
    #[error("custom resource kind `{input}` must look like custom:<plugin>/<name>")]
    CustomShape { input: String },
}

impl TryFrom<String> for ResourceKind {
    type Error = ResourceKindError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ResourceKind> for String {
    fn from(kind: ResourceKind) -> Self {
        kind.to_string()
    }
}

/// Something that flows through the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub id: Id,
    pub kind: ResourceKind,
    /// Translation key, e.g. `item.minecraft.iron_ingot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_key: Option<String>,
    /// Texture reference, e.g. `minecraft:item/iron_ingot`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Display unit, for `energy` kinds that measure in FE, J and so on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
}

/// A group of interchangeable resources, written `#c:ingots/iron`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub id: Id,
    #[serde(default)]
    pub values: Vec<Id>,
}

/// Either a concrete resource or a tag standing in for several.
///
/// Recipe inputs may be written either way; the leading `#` picks the tag.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ResourceRef {
    Resource(Id),
    Tag(Id),
}

impl ResourceRef {
    /// The underlying id, whichever side this is.
    #[must_use]
    pub const fn id(&self) -> &Id {
        match self {
            Self::Resource(id) | Self::Tag(id) => id,
        }
    }

    #[must_use]
    pub const fn is_tag(&self) -> bool {
        matches!(self, Self::Tag(_))
    }
}

impl fmt::Display for ResourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resource(id) => write!(f, "{id}"),
            Self::Tag(id) => write!(f, "#{id}"),
        }
    }
}

impl FromStr for ResourceRef {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input.strip_prefix('#') {
            Some(rest) => rest.parse().map(Self::Tag),
            None => input.parse().map(Self::Resource),
        }
    }
}

impl TryFrom<String> for ResourceRef {
    type Error = IdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<ResourceRef> for String {
    fn from(value: ResourceRef) -> Self {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::{Resource, ResourceKind, ResourceKindError, ResourceRef};

    #[test]
    fn kinds_round_trip() {
        for text in ["item", "fluid", "energy", "stress", "custom:bees/genes"] {
            let kind: ResourceKind = text.parse().expect("valid kind");
            assert_eq!(kind.to_string(), text);
        }
    }

    #[test]
    fn custom_kind_is_split() {
        let kind: ResourceKind = "custom:bees/genes".parse().expect("valid kind");
        assert_eq!(
            kind,
            ResourceKind::Custom {
                plugin: "bees".to_owned(),
                name: "genes".to_owned(),
            }
        );
    }

    #[test]
    fn bad_kinds_are_rejected() {
        assert!(matches!(
            "gas".parse::<ResourceKind>(),
            Err(ResourceKindError::Unknown { .. })
        ));
        assert!(matches!(
            "custom:bees".parse::<ResourceKind>(),
            Err(ResourceKindError::CustomShape { .. })
        ));
    }

    #[test]
    fn hash_marks_a_tag() {
        let tag: ResourceRef = "#c:ingots/iron".parse().expect("valid ref");
        assert!(tag.is_tag());
        assert_eq!(tag.to_string(), "#c:ingots/iron");

        let plain: ResourceRef = "minecraft:iron_ingot".parse().expect("valid ref");
        assert!(!plain.is_tag());
        assert_eq!(plain.to_string(), "minecraft:iron_ingot");
    }

    #[test]
    fn resource_matches_the_design_example() {
        let json = r#"{
            "id": "minecraft:iron_ingot",
            "kind": "item",
            "name_key": "item.minecraft.iron_ingot",
            "icon": "minecraft:item/iron_ingot"
        }"#;
        let resource: Resource = serde_json::from_str(json).expect("parses");
        assert_eq!(resource.kind, ResourceKind::Item);
        assert_eq!(resource.id.to_string(), "minecraft:iron_ingot");
        assert_eq!(resource.unit, None);
    }
}
