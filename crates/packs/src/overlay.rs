//! Overlays: edits applied on top of packs that are already loaded.
//!
//! An overlay file is a list of operations, each naming the entity it touches
//! and, for `set` and a field-level `remove`, a dotted path inside it:
//!
//! ```json
//! {
//!   "operations": [
//!     { "op": "set", "target": "machine:minecraft:furnace",
//!       "path": "constants.base_energy", "value": 60 },
//!     { "op": "remove", "target": "recipe:minecraft:smelting/glass" },
//!     { "op": "add", "kind": "resource",
//!       "value": { "id": "user:scrap", "kind": "item" } }
//!   ]
//! }
//! ```
//!
//! Operations work on raw JSON rather than on the typed model, which is what
//! lets a single number be changed without restating the whole machine.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::PackError;

/// Which collection an entity lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Resource,
    Tag,
    Machine,
    Recipe,
}

impl EntityKind {
    pub const ALL: [Self; 4] = [Self::Resource, Self::Tag, Self::Machine, Self::Recipe];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resource => "resource",
            Self::Tag => "tag",
            Self::Machine => "machine",
            Self::Recipe => "recipe",
        }
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EntityKind {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "resource" => Ok(Self::Resource),
            "tag" => Ok(Self::Tag),
            "machine" => Ok(Self::Machine),
            "recipe" => Ok(Self::Recipe),
            _ => Err(()),
        }
    }
}

/// `machine:mekanism:enrichment_chamber`: a kind plus the entity's own id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityRef {
    pub kind: EntityKind,
    pub id: model::Id,
}

impl fmt::Display for EntityRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind, self.id)
    }
}

impl EntityRef {
    /// Splits off the kind, leaving the `namespace:path` id behind.
    pub fn parse(text: &str, path: &Path) -> Result<Self, PackError> {
        let bad = |reason: &str| PackError::OverlayTarget {
            path: path.to_path_buf(),
            target: text.to_owned(),
            reason: reason.to_owned(),
        };

        let (kind, id) = text
            .split_once(':')
            .ok_or_else(|| bad("there is no `:` after the kind"))?;

        let kind: EntityKind = kind
            .parse()
            .map_err(|()| bad("the kind must be one of resource, tag, machine or recipe"))?;
        let id: model::Id = id.parse().map_err(|e| bad(&format!("{e}")))?;

        Ok(Self { kind, id })
    }
}

/// One edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    /// Writes `value` at `path` inside an existing entity.
    Set {
        target: String,
        path: String,
        value: Value,
    },
    /// Without `path`, drops the whole entity; with one, drops that field.
    Remove {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
    },
    /// Introduces a new entity.
    Add { kind: EntityKind, value: Value },
}

/// The contents of one `overlays/*.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Overlay {
    #[serde(default)]
    pub operations: Vec<Operation>,
}

/// One segment of a dotted path: an object key or an array index.
enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

fn segments(path: &str) -> Vec<Segment<'_>> {
    path.split('.')
        .map(|part| match part.parse::<usize>() {
            Ok(index) => Segment::Index(index),
            Err(_) => Segment::Key(part),
        })
        .collect()
}

/// Writes `value` at `path`, creating missing objects along the way.
///
/// Numeric segments index into arrays, so `outputs.0.amount` works; the array
/// itself must already exist, since there is no sensible way to invent one.
pub(crate) fn set_path(root: &mut Value, path: &str, value: Value) -> Result<(), String> {
    let parts = segments(path);
    let Some((last, parents)) = parts.split_last() else {
        return Err("the path is empty".to_owned());
    };

    let mut cursor = root;
    for (depth, segment) in parents.iter().enumerate() {
        cursor = match segment {
            Segment::Key(key) => {
                if !cursor.is_object() {
                    return Err(describe(path, depth, "is not an object"));
                }
                let Some(map) = cursor.as_object_mut() else {
                    return Err(describe(path, depth, "is not an object"));
                };
                map.entry((*key).to_owned())
                    .or_insert_with(|| Value::Object(Map::new()))
            }
            Segment::Index(index) => {
                let Some(array) = cursor.as_array_mut() else {
                    return Err(describe(path, depth, "is not an array"));
                };
                array
                    .get_mut(*index)
                    .ok_or_else(|| describe(path, depth, "is past the end of the array"))?
            }
        };
    }

    match last {
        Segment::Key(key) => {
            let Some(map) = cursor.as_object_mut() else {
                return Err(describe(path, parts.len() - 1, "is not an object"));
            };
            map.insert((*key).to_owned(), value);
        }
        Segment::Index(index) => {
            let Some(array) = cursor.as_array_mut() else {
                return Err(describe(path, parts.len() - 1, "is not an array"));
            };
            let slot = array
                .get_mut(*index)
                .ok_or_else(|| describe(path, parts.len() - 1, "is past the end of the array"))?;
            *slot = value;
        }
    }

    Ok(())
}

/// Drops the field at `path`. A missing field is not an error: the edit was
/// going to leave it absent either way.
pub(crate) fn remove_path(root: &mut Value, path: &str) -> Result<(), String> {
    let parts = segments(path);
    let Some((last, parents)) = parts.split_last() else {
        return Err("the path is empty".to_owned());
    };

    let mut cursor = root;
    for (depth, segment) in parents.iter().enumerate() {
        let next = match segment {
            Segment::Key(key) => cursor.get_mut(*key),
            Segment::Index(index) => cursor.get_mut(*index),
        };
        match next {
            Some(value) => cursor = value,
            // Nothing there to remove.
            None => return Ok(()),
        }
        let _ = depth;
    }

    match last {
        Segment::Key(key) => {
            if let Some(map) = cursor.as_object_mut() {
                map.remove(*key);
            }
        }
        Segment::Index(index) => {
            if let Some(array) = cursor.as_array_mut()
                && *index < array.len()
            {
                array.remove(*index);
            }
        }
    }

    Ok(())
}

fn describe(path: &str, depth: usize, problem: &str) -> String {
    let prefix: Vec<&str> = path.split('.').take(depth + 1).collect();
    format!("`{}` {problem}", prefix.join("."))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Overlay, remove_path, set_path};

    #[test]
    fn sets_a_nested_value() {
        let mut value = json!({ "constants": { "base_energy": 50 } });
        set_path(&mut value, "constants.base_energy", json!(60)).expect("sets");
        assert_eq!(value["constants"]["base_energy"], json!(60));
    }

    #[test]
    fn creates_missing_objects_on_the_way() {
        let mut value = json!({});
        set_path(&mut value, "constants.base_energy", json!(60)).expect("sets");
        assert_eq!(value, json!({ "constants": { "base_energy": 60 } }));
    }

    #[test]
    fn indexes_into_arrays() {
        let mut value = json!({ "outputs": [ { "amount": 1 }, { "amount": 2 } ] });
        set_path(&mut value, "outputs.1.amount", json!(5)).expect("sets");
        assert_eq!(value["outputs"][1]["amount"], json!(5));
    }

    #[test]
    fn reports_a_path_that_does_not_fit() {
        let mut value = json!({ "outputs": [ { "amount": 1 } ] });
        let err = set_path(&mut value, "outputs.7.amount", json!(5)).expect_err("rejects");
        assert!(err.contains("past the end"), "got {err}");

        let mut value = json!({ "duration": 200 });
        let err = set_path(&mut value, "duration.0", json!(5)).expect_err("rejects");
        assert!(err.contains("not an array"), "got {err}");
    }

    #[test]
    fn removes_fields_and_tolerates_absent_ones() {
        let mut value = json!({ "constants": { "a": 1, "b": 2 } });
        remove_path(&mut value, "constants.a").expect("removes");
        assert_eq!(value, json!({ "constants": { "b": 2 } }));

        remove_path(&mut value, "constants.nope").expect("tolerates");
        remove_path(&mut value, "missing.deeply.nested").expect("tolerates");
    }

    #[test]
    fn parses_the_documented_file_shape() {
        let text = r#"{
            "operations": [
                { "op": "set", "target": "machine:minecraft:furnace",
                  "path": "constants.base_energy", "value": 60 },
                { "op": "remove", "target": "recipe:minecraft:smelting/glass" },
                { "op": "add", "kind": "resource",
                  "value": { "id": "user:scrap", "kind": "item" } }
            ]
        }"#;
        let overlay: Overlay = serde_json::from_str(text).expect("parses");
        assert_eq!(overlay.operations.len(), 3);
    }
}
