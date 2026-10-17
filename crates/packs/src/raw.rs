//! Reading a pack directory off disk, without interpreting it yet.
//!
//! Entities stay as `serde_json::Value` at this stage. Layering and overlays
//! both work on raw JSON, and only the merged result is turned into model
//! types — that is what lets an overlay change one number without restating
//! the whole entity.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use expr::FunctionDef;
use model::Id;
use serde::Deserialize;
use serde_json::Value;

use crate::error::PackError;
use crate::manifest::PackManifest;
use crate::overlay::{EntityKind, Overlay};

/// One entity as read from a file, with enough context to blame it later.
#[derive(Debug, Clone)]
pub struct Entry {
    pub value: Value,
    pub pack: String,
    pub file: PathBuf,
}

/// A pack directory, read but not yet merged with anything.
#[derive(Debug, Clone)]
pub struct RawPack {
    pub manifest: PackManifest,
    pub root: PathBuf,
    pub entities: BTreeMap<EntityKind, BTreeMap<Id, Entry>>,
    pub functions: Vec<(String, FunctionDef)>,
    pub overlays: Vec<(PathBuf, Overlay)>,
}

/// A function as written in `functions.json`.
#[derive(Debug, Clone, Deserialize)]
struct FunctionFile {
    #[serde(default)]
    params: Vec<String>,
    body: String,
}

impl RawPack {
    /// Reads `root`, which must contain a `pack.toml`.
    pub fn load(root: impl AsRef<Path>) -> Result<Self, PackError> {
        let root = root.as_ref().to_path_buf();
        let manifest = PackManifest::load(&root.join("pack.toml"))?;

        let mut entities = BTreeMap::new();
        for kind in EntityKind::ALL {
            let mut map = BTreeMap::new();
            // Single files: resources.json, tags.json, machines.json.
            let single = root.join(format!("{}s.json", kind.as_str()));
            if single.is_file() {
                read_entities(&single, &manifest.id, &mut map)?;
            }
            // Directories: recipes/**.json, and the same option for the rest.
            let directory = root.join(format!("{}s", kind.as_str()));
            if directory.is_dir() {
                for file in json_files(&directory)? {
                    read_entities(&file, &manifest.id, &mut map)?;
                }
            }
            entities.insert(kind, map);
        }

        let functions = read_functions(&root.join("functions.json"))?;

        let mut overlays = Vec::new();
        let overlay_dir = root.join("overlays");
        if overlay_dir.is_dir() {
            for file in json_files(&overlay_dir)? {
                let text = std::fs::read_to_string(&file).map_err(|e| PackError::io(&file, e))?;
                let overlay: Overlay =
                    serde_json::from_str(&text).map_err(|source| PackError::Json {
                        path: file.clone(),
                        source,
                    })?;
                overlays.push((file, overlay));
            }
        }

        Ok(Self {
            manifest,
            root,
            entities,
            functions,
            overlays,
        })
    }
}

/// Every `.json` under `dir`, recursively, in a stable order.
fn json_files(dir: &Path) -> Result<Vec<PathBuf>, PackError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|e| PackError::io(&current, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| PackError::io(&current, e))?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "json") {
                out.push(path);
            }
        }
    }

    out.sort();
    Ok(out)
}

/// Reads a file holding either one entity or an array of them.
fn read_entities(file: &Path, pack: &str, into: &mut BTreeMap<Id, Entry>) -> Result<(), PackError> {
    let text = std::fs::read_to_string(file).map_err(|e| PackError::io(file, e))?;
    let parsed: Value = serde_json::from_str(&text).map_err(|source| PackError::Json {
        path: file.to_path_buf(),
        source,
    })?;

    let values = match parsed {
        Value::Array(items) => items,
        other => vec![other],
    };

    for value in values {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| PackError::Overlay {
                path: file.to_path_buf(),
                message: "an entity has no `id` field".to_owned(),
            })?;
        let id: Id = id.parse().map_err(|e| PackError::Overlay {
            path: file.to_path_buf(),
            message: format!("{e}"),
        })?;

        into.insert(
            id,
            Entry {
                value,
                pack: pack.to_owned(),
                file: file.to_path_buf(),
            },
        );
    }

    Ok(())
}

/// `functions.json`: a map of name to `{ "params": [...], "body": "..." }`.
fn read_functions(file: &Path) -> Result<Vec<(String, FunctionDef)>, PackError> {
    if !file.is_file() {
        return Ok(Vec::new());
    }

    let text = std::fs::read_to_string(file).map_err(|e| PackError::io(file, e))?;
    let parsed: BTreeMap<String, FunctionFile> =
        serde_json::from_str(&text).map_err(|source| PackError::Json {
            path: file.to_path_buf(),
            source,
        })?;

    Ok(parsed
        .into_iter()
        .map(|(name, function)| {
            let def = FunctionDef::new(name.clone(), function.params, function.body);
            (name, def)
        })
        .collect())
}
