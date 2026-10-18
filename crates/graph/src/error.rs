//! Errors that stop a schema file from being read at all.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("not valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("the file has no `format_version`")]
    NoFormatVersion,

    #[error(
        "this schema is version {found}, but this build only reads up to {supported}; update Craftflow"
    )]
    TooNew { found: u32, supported: u32 },

    #[error("schema version {found} is too old to upgrade: there is no migration from it")]
    NoMigration { found: u32 },

    #[error("migrating from version {from}: {message}")]
    Migration { from: u32, message: String },
}
