//! Namespaced identifiers (DESIGN.md §4): `minecraft:iron_ingot`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{IdError, IdPart};

/// A `namespace:path` identifier, in Minecraft's own style.
///
/// Serialised as the plain string `"namespace:path"`, which is why the
/// `try_from`/`into` attributes are here rather than a derived struct shape.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Id {
    namespace: String,
    path: String,
}

impl Id {
    /// Builds an id from its two halves, validating both.
    pub fn new(namespace: impl Into<String>, path: impl Into<String>) -> Result<Self, IdError> {
        let namespace = namespace.into();
        let path = path.into();
        let input = format!("{namespace}:{path}");

        check_part(&input, IdPart::Namespace, &namespace)?;
        check_part(&input, IdPart::Path, &path)?;

        Ok(Self { namespace, path })
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// Minecraft allows `.`, `-`, `_` and lowercase alphanumerics everywhere, plus
/// `/` in paths only.
fn check_part(input: &str, part: IdPart, value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        return Err(IdError::Empty {
            input: input.to_owned(),
            part,
        });
    }

    for ch in value.chars() {
        let ok = matches!(ch, 'a'..='z' | '0'..='9' | '_' | '-' | '.')
            || (ch == '/' && part == IdPart::Path);
        if !ok {
            return Err(IdError::char(input, part, ch));
        }
    }

    Ok(())
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad` rather than `write!`, so that `{:<32}` lines ids up in a
        // column; a plain `write!` silently ignores width and alignment.
        f.pad(&format!("{}:{}", self.namespace, self.path))
    }
}

impl FromStr for Id {
    type Err = IdError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        // Exactly one `:`; a bare `iron_ingot` is rejected rather than being
        // silently promoted to `minecraft:iron_ingot`.
        let Some((namespace, path)) = input.split_once(':') else {
            return Err(IdError::Shape {
                input: input.to_owned(),
            });
        };
        if path.contains(':') {
            return Err(IdError::Shape {
                input: input.to_owned(),
            });
        }

        check_part(input, IdPart::Namespace, namespace)?;
        check_part(input, IdPart::Path, path)?;

        Ok(Self {
            namespace: namespace.to_owned(),
            path: path.to_owned(),
        })
    }
}

impl TryFrom<String> for Id {
    type Error = IdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<Id> for String {
    fn from(id: Id) -> Self {
        id.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::Id;
    use crate::error::{IdError, IdPart};

    #[test]
    fn parses_and_round_trips() {
        let id: Id = "minecraft:iron_ingot".parse().expect("valid id");
        assert_eq!(id.namespace(), "minecraft");
        assert_eq!(id.path(), "iron_ingot");
        assert_eq!(id.to_string(), "minecraft:iron_ingot");
    }

    #[test]
    fn allows_slashes_in_the_path_only() {
        assert!("mekanism:enriching/iron_ore".parse::<Id>().is_ok());
        assert!(matches!(
            "me/kanism:x".parse::<Id>(),
            Err(IdError::Char {
                part: IdPart::Namespace,
                ch: '/',
                ..
            })
        ));
    }

    #[test]
    fn rejects_missing_or_extra_separators() {
        assert!(matches!(
            "iron_ingot".parse::<Id>(),
            Err(IdError::Shape { .. })
        ));
        assert!(matches!("a:b:c".parse::<Id>(), Err(IdError::Shape { .. })));
    }

    #[test]
    fn rejects_empty_halves_and_uppercase() {
        assert!(matches!(
            ":path".parse::<Id>(),
            Err(IdError::Empty {
                part: IdPart::Namespace,
                ..
            })
        ));
        assert!(matches!(
            "ns:".parse::<Id>(),
            Err(IdError::Empty {
                part: IdPart::Path,
                ..
            })
        ));
        assert!(matches!(
            "Minecraft:iron".parse::<Id>(),
            Err(IdError::Char { ch: 'M', .. })
        ));
    }

    #[test]
    fn serialises_as_a_plain_string() {
        let id: Id = "minecraft:stone".parse().expect("valid id");
        let json = serde_json::to_string(&id).expect("serialises");
        assert_eq!(json, "\"minecraft:stone\"");
        assert_eq!(serde_json::from_str::<Id>(&json).expect("round trips"), id);
    }
}
