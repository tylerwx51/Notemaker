//! Identifiers: Java `UUID` helpers and the string newtypes that name
//! documents, actors, sessions, layers and drawing entities.
//!
//! Each identifier keeps the exact string it was created from, so JSON
//! output and error messages are unchanged; the type only records what the
//! string identifies (ADR 0030).

use md5::{Digest, Md5};

/// Defines a string newtype with conversions to and from `String`, `&str`
/// and JSON values. Use it for identifiers and names whose meaning should be
/// visible in signatures. It derefs to `str`, so `&id` passes where a `&str`
/// is expected; going the other way needs an explicit `From`.
#[macro_export]
macro_rules! string_newtype {
    ($(#[$meta:meta])* $vis:vis struct $name:ident;) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                $name(value.into())
            }
        }
        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }
        impl From<String> for $name {
            fn from(value: String) -> Self {
                $name(value)
            }
        }
        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                $name(value.to_string())
            }
        }
        impl From<&String> for $name {
            fn from(value: &String) -> Self {
                $name(value.clone())
            }
        }
        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }
        impl From<$name> for $crate::json::Value {
            fn from(value: $name) -> $crate::json::Value {
                $crate::json::Value::String(value.0)
            }
        }
        impl From<&$name> for $crate::json::Value {
            fn from(value: &$name) -> $crate::json::Value {
                $crate::json::Value::String(value.0.clone())
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0 == other
            }
        }
        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0 == *other
            }
        }
    };
}

/// A UUID in Java's `UUID.toString()` form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Uuid(String);

impl Uuid {
    /// `UUID.randomUUID()` (also `Actions.id()`).
    pub fn random() -> Uuid {
        Uuid(uuid::Uuid::new_v4().to_string())
    }

    /// `UUID.nameUUIDFromBytes(name)`: MD5, version 3 bits, no namespace.
    pub fn from_name(name: impl AsRef<[u8]>) -> Uuid {
        let mut md5: [u8; 16] = Md5::digest(name.as_ref()).into();
        md5[6] &= 0x0f;
        md5[6] |= 0x30;
        md5[8] &= 0x3f;
        md5[8] |= 0x80;
        Uuid(uuid::Uuid::from_bytes(md5).to_string())
    }

}

impl std::ops::Deref for Uuid {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Uuid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<Uuid> for String {
    fn from(value: Uuid) -> String {
        value.0
    }
}

impl From<Uuid> for crate::json::Value {
    fn from(value: Uuid) -> crate::json::Value {
        crate::json::Value::String(value.0)
    }
}

string_newtype! {
    /// The actor whose edits and undo history an operation belongs to.
    pub struct ActorId;
}

string_newtype! {
    /// One editing session of an actor (a selection lease owner).
    pub struct SessionId;
}

string_newtype! {
    /// A document's name, which also names its journal file.
    pub struct DocumentName;
}

string_newtype! {
    /// A layer's id (`"base"` for the default layer).
    pub struct LayerId;
}

string_newtype! {
    /// The id of a stroke or object.
    pub struct EntityId;
}

string_newtype! {
    /// The entity table a reference points into: `"stroke"` or `"object"`.
    /// Kept as the original string so unknown kinds fail where the Java code
    /// failed.
    pub struct EntityKind;
}

string_newtype! {
    /// `Collaboration.key(kind, id, page)`: `page:kind:id`, the key of a
    /// selection lease.
    pub struct EntityKey;
}

string_newtype! {
    /// The id of a recorded sync conflict.
    pub struct ConflictId;
}

string_newtype! {
    /// A shape catalog id such as `"rect"` or `"arrow"`.
    pub struct ShapeId;
}

impl EntityId {
    /// A fresh random id.
    pub fn random() -> EntityId {
        EntityId(String::from(Uuid::random()))
    }
}

impl LayerId {
    /// The default layer.
    pub fn base() -> LayerId {
        LayerId("base".to_string())
    }

    /// A fresh random id.
    pub fn random() -> LayerId {
        LayerId(String::from(Uuid::random()))
    }
}

impl SessionId {
    /// A fresh random id.
    pub fn random() -> SessionId {
        SessionId(String::from(Uuid::random()))
    }
}

impl EntityKind {
    pub fn stroke() -> EntityKind {
        EntityKind("stroke".to_string())
    }

    pub fn object() -> EntityKind {
        EntityKind("object".to_string())
    }

    pub fn is_stroke(&self) -> bool {
        self.0 == "stroke"
    }

    pub fn is_object(&self) -> bool {
        self.0 == "object"
    }
}

/// How to resolve a sync conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictChoice {
    /// Keep the local version.
    Local,
    /// Keep the remote version.
    Remote,
    /// Keep the local version and add a copy of the remote one.
    Both,
}

impl ConflictChoice {
    /// Parses `local`, `remote` or `both`, with the Java error message.
    pub fn parse(choice: &ConflictChoiceName) -> crate::Result<ConflictChoice> {
        match &**choice {
            "local" => Ok(ConflictChoice::Local),
            "remote" => Ok(ConflictChoice::Remote),
            "both" => Ok(ConflictChoice::Both),
            _ => Err(crate::Error::illegal_argument("Conflict choice must be local, remote, or both")),
        }
    }
}

string_newtype! {
    /// A conflict choice as typed by a user, before [`ConflictChoice::parse`].
    pub struct ConflictChoiceName;
}

/// A layer's position in the drawing order, `0` for the bottom layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerPosition(pub i32);

/// A page index within a document (`0` for single-canvas documents).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageNumber(pub i32);

impl PageNumber {
    pub const FIRST: PageNumber = PageNumber(0);
}

impl std::fmt::Display for PageNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<PageNumber> for crate::json::Value {
    fn from(value: PageNumber) -> crate::json::Value {
        crate::json::Value::from(value.0)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn name_uuid() {
        // UUID.nameUUIDFromBytes("doc".getBytes(UTF_8))
        assert_eq!(super::Uuid::from_name(b"doc").len(), 36);
    }
}
