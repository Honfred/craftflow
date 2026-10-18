//! Schema file versioning (DESIGN.md §10).
//!
//! A file is upgraded one version at a time, `vN -> vN+1`, before it is
//! deserialised. Adding a version means appending one function here and a
//! test that exercises it; the deserialising code then only ever sees the
//! current shape.

use serde_json::Value;

use crate::error::GraphError;

/// The schema version this build writes.
pub const CURRENT_FORMAT_VERSION: u32 = 1;

/// Turns a document of version `from` into one of version `from + 1`.
type Step = fn(&mut Value) -> Result<(), String>;

/// One entry per upgrade, keyed by the version it reads.
///
/// Empty while version 1 is the only one that has ever existed.
const MIGRATIONS: &[(u32, Step)] = &[];

/// Reads the `format_version` field.
pub fn format_version(document: &Value) -> Result<u32, GraphError> {
    document
        .get("format_version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(GraphError::NoFormatVersion)
}

/// Brings a document up to [`CURRENT_FORMAT_VERSION`].
pub fn migrate(mut document: Value) -> Result<Value, GraphError> {
    let mut version = format_version(&document)?;

    if version > CURRENT_FORMAT_VERSION {
        return Err(GraphError::TooNew {
            found: version,
            supported: CURRENT_FORMAT_VERSION,
        });
    }

    while version < CURRENT_FORMAT_VERSION {
        let Some((_, step)) = MIGRATIONS.iter().find(|(from, _)| *from == version) else {
            return Err(GraphError::NoMigration { found: version });
        };
        step(&mut document).map_err(|message| GraphError::Migration {
            from: version,
            message,
        })?;
        version += 1;
        document["format_version"] = Value::from(version);
    }

    Ok(document)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CURRENT_FORMAT_VERSION, migrate};
    use crate::error::GraphError;

    #[test]
    fn a_current_document_passes_through_unchanged() {
        let document = json!({ "format_version": CURRENT_FORMAT_VERSION, "nodes": [] });
        assert_eq!(migrate(document.clone()).expect("migrates"), document);
    }

    #[test]
    fn a_newer_document_is_refused_rather_than_guessed_at() {
        let document = json!({ "format_version": CURRENT_FORMAT_VERSION + 1 });
        assert!(matches!(migrate(document), Err(GraphError::TooNew { .. })));
    }

    #[test]
    fn a_version_with_no_upgrade_path_says_so() {
        let document = json!({ "format_version": 0 });
        assert!(matches!(
            migrate(document),
            Err(GraphError::NoMigration { found: 0 })
        ));
    }

    #[test]
    fn a_document_without_a_version_is_refused() {
        assert!(matches!(
            migrate(json!({ "nodes": [] })),
            Err(GraphError::NoFormatVersion)
        ));
    }
}
