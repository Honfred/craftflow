//! `pack.toml` (DESIGN.md §5).

use std::path::Path;

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};

use crate::error::PackError;

/// The only pack format this build understands.
pub const SUPPORTED_FORMAT_VERSION: u32 = 1;

/// A `pack.toml`, which wraps everything in a `[pack]` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFile {
    pack: PackManifest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackManifest {
    pub id: String,
    pub version: String,
    pub format_version: u32,
    /// Minecraft version this pack was written against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minecraft: Option<String>,
    /// Mod loader, e.g. `neoforge`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader: Option<String>,
    /// Entries like `vanilla >= 0.1`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends: Vec<String>,
}

impl PackManifest {
    /// Reads and checks a `pack.toml`.
    pub fn load(path: &Path) -> Result<Self, PackError> {
        let text = std::fs::read_to_string(path).map_err(|source| PackError::io(path, source))?;
        let file: ManifestFile = toml::from_str(&text).map_err(|source| PackError::Toml {
            path: path.to_path_buf(),
            source,
        })?;
        let manifest = file.pack;

        if manifest.format_version != SUPPORTED_FORMAT_VERSION {
            return Err(PackError::FormatVersion {
                path: path.to_path_buf(),
                found: manifest.format_version,
                supported: SUPPORTED_FORMAT_VERSION,
            });
        }

        // Parse both now so a malformed version fails at load rather than
        // halfway through dependency resolution.
        manifest.semver(path)?;
        for requirement in &manifest.depends {
            Dependency::parse(requirement, path)?;
        }

        Ok(manifest)
    }

    /// The pack's own version.
    pub fn semver(&self, path: &Path) -> Result<Version, PackError> {
        Version::parse(&self.version).map_err(|source| PackError::Version {
            path: path.to_path_buf(),
            version: self.version.clone(),
            source,
        })
    }

    pub fn dependencies(&self, path: &Path) -> Result<Vec<Dependency>, PackError> {
        self.depends
            .iter()
            .map(|requirement| Dependency::parse(requirement, path))
            .collect()
    }
}

/// One entry of `depends`: a pack id and a version requirement.
#[derive(Debug, Clone, PartialEq)]
pub struct Dependency {
    pub id: String,
    pub requirement: VersionReq,
    /// The original text, for error messages.
    pub source: String,
}

impl Dependency {
    /// Parses `vanilla >= 0.1`, or a bare `vanilla` meaning any version.
    pub fn parse(text: &str, path: &Path) -> Result<Self, PackError> {
        let text = text.trim();
        let bad = |reason: &str| PackError::Dependency {
            path: path.to_path_buf(),
            requirement: text.to_owned(),
            reason: reason.to_owned(),
        };

        let (id, rest) = match text.find(|c: char| c.is_whitespace()) {
            Some(index) => text.split_at(index),
            None => (text, ""),
        };

        if id.is_empty() {
            return Err(bad("it has no pack id"));
        }

        let rest = rest.trim();
        let requirement = if rest.is_empty() {
            VersionReq::STAR
        } else {
            VersionReq::parse(rest).map_err(|source| bad(&source.to_string()))?
        };

        Ok(Self {
            id: id.to_owned(),
            requirement,
            source: text.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Dependency;

    #[test]
    fn parses_an_id_and_a_requirement() {
        let dep = Dependency::parse("vanilla >= 0.1", Path::new("pack.toml")).expect("parses");
        assert_eq!(dep.id, "vanilla");
        assert!(dep.requirement.matches(&semver::Version::new(0, 1, 0)));
        assert!(dep.requirement.matches(&semver::Version::new(1, 0, 0)));
        assert!(!dep.requirement.matches(&semver::Version::new(0, 0, 9)));
    }

    #[test]
    fn a_bare_id_accepts_any_version() {
        let dep = Dependency::parse("vanilla", Path::new("pack.toml")).expect("parses");
        assert_eq!(dep.id, "vanilla");
        assert!(dep.requirement.matches(&semver::Version::new(9, 9, 9)));
    }

    #[test]
    fn rejects_nonsense() {
        assert!(Dependency::parse("vanilla >= wat", Path::new("pack.toml")).is_err());
        assert!(Dependency::parse("", Path::new("pack.toml")).is_err());
    }
}
