//! Things that stop a pack from loading at all.
//!
//! Problems *inside* an otherwise readable set of packs — a recipe naming a
//! resource nobody defines — are [`crate::validate::Issue`]s instead, so that
//! one broken recipe does not hide the rest.

use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PackError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("{path}: pack format version {found} is not supported (this build reads {supported})")]
    FormatVersion {
        path: PathBuf,
        found: u32,
        supported: u32,
    },

    #[error("{path}: `{version}` is not a valid version: {source}")]
    Version {
        path: PathBuf,
        version: String,
        #[source]
        source: semver::Error,
    },

    #[error("{path}: `{requirement}` is not a valid dependency: {reason}")]
    Dependency {
        path: PathBuf,
        requirement: String,
        reason: String,
    },

    #[error("pack `{pack}` depends on `{dependency}`, which is not loaded")]
    MissingDependency { pack: String, dependency: String },

    #[error(
        "pack `{pack}` needs `{dependency} {requirement}`, but version {found} is what is loaded"
    )]
    UnsatisfiedDependency {
        pack: String,
        dependency: String,
        requirement: String,
        found: String,
    },

    #[error("packs depend on one another in a cycle: {}", .cycle.join(" -> "))]
    DependencyCycle { cycle: Vec<String> },

    #[error("two packs share the id `{id}`")]
    DuplicatePack { id: String },

    #[error("{path}: `{target}` should look like `<kind>:<namespace>:<path>`, {reason}")]
    OverlayTarget {
        path: PathBuf,
        target: String,
        reason: String,
    },

    #[error("{path}: {message}")]
    Overlay { path: PathBuf, message: String },
}

impl PackError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
