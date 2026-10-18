//! Data pack loading: registry, dependencies, overlays, validation.
//!
//! See `docs/DESIGN.md` §5. A pack directory looks like this:
//!
//! ```text
//! vanilla/
//! ├── pack.toml
//! ├── resources.json
//! ├── tags.json
//! ├── machines.json
//! ├── functions.json
//! ├── recipes/*.json
//! └── overlays/*.json
//! ```
//!
//! Loading happens in three steps. Each pack is read into raw JSON, the packs
//! are layered in dependency order with overlays applied on top, and only
//! then is the result turned into model types and checked. Keeping the middle
//! step on raw JSON is what lets an overlay change one number without
//! restating the entity it belongs to.

// Rule 5: no `unwrap`/`expect` in library code, except provably impossible
// cases carrying a comment explaining why. Tests are exempt.
#![warn(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod error;
pub mod manifest;
pub mod overlay;
pub mod raw;
pub mod registry;
pub mod validate;

pub use error::PackError;
pub use manifest::{Dependency, PackManifest, SUPPORTED_FORMAT_VERSION};
pub use model::{Issue, Severity};
pub use overlay::{EntityKind, EntityRef, Operation, Overlay};
pub use raw::RawPack;
pub use registry::{Loaded, RECIPE_FIELDS, Registry, build};

use std::path::Path;

/// Reads every given directory as a pack and layers them together.
pub fn load(roots: impl IntoIterator<Item = impl AsRef<Path>>) -> Result<Loaded, PackError> {
    let packs = roots
        .into_iter()
        .map(RawPack::load)
        .collect::<Result<Vec<_>, _>>()?;
    build(packs)
}
