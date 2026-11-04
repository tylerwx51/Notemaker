//! Named JSON shapes: thin newtypes over `serde_json` maps, arrays and values
//! (ADR 0032).
//!
//! Operations, replica states, entity content and command results cross the
//! journal, the network and the CLI as JSON that must stay byte-compatible
//! with the Java reference. Each newtype keeps the JSON exactly as it is (key
//! order, unknown keys, number formatting) and only names what it holds, so
//! signatures say which JSON goes where. They dereference to the JSON they
//! wrap.
//!
//! Every wrapper is `#[repr(transparent)]`, so a borrowed map, slice or value
//! converts to a borrowed wrapper without copying: `let op: &Operation =
//! map.into();`. Owned values convert with `From`, and JSON values convert
//! fallibly with `TryFrom` (the error is `Json.map`'s "Expected JSON object"
//! or `Json.list`'s "Expected JSON array").

/// Defines a newtype over a JSON object (`Map`). See the module docs.
#[macro_export]
macro_rules! json_object_newtype {
    ($(#[$meta:meta])* $vis:vis struct $name:ident;) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq)]
        #[repr(transparent)]
        $vis struct $name(pub $crate::json::Map);

        impl std::ops::Deref for $name {
            type Target = $crate::json::Map;
            fn deref(&self) -> &$crate::json::Map {
                &self.0
            }
        }
        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut $crate::json::Map {
                &mut self.0
            }
        }
        impl AsRef<$crate::json::Map> for $name {
            fn as_ref(&self) -> &$crate::json::Map {
                &self.0
            }
        }
        impl From<$crate::json::Map> for $name {
            fn from(value: $crate::json::Map) -> Self {
                $name(value)
            }
        }
        impl From<$name> for $crate::json::Map {
            fn from(value: $name) -> $crate::json::Map {
                value.0
            }
        }
        impl From<$name> for $crate::json::Value {
            fn from(value: $name) -> $crate::json::Value {
                $crate::json::Value::Object(value.0)
            }
        }
        impl TryFrom<$crate::json::Value> for $name {
            type Error = $crate::Error;
            fn try_from(value: $crate::json::Value) -> $crate::Result<Self> {
                match value {
                    $crate::json::Value::Object(map) => Ok($name(map)),
                    _ => Err($crate::Error::illegal_argument("Expected JSON object")),
                }
            }
        }
        impl<'a> From<&'a $crate::json::Map> for &'a $name {
            fn from(value: &'a $crate::json::Map) -> Self {
                // SAFETY: `$name` is `#[repr(transparent)]` over `Map`.
                unsafe { &*(value as *const $crate::json::Map as *const $name) }
            }
        }
        impl<'a> From<&'a mut $crate::json::Map> for &'a mut $name {
            fn from(value: &'a mut $crate::json::Map) -> Self {
                // SAFETY: `$name` is `#[repr(transparent)]` over `Map`.
                unsafe { &mut *(value as *mut $crate::json::Map as *mut $name) }
            }
        }
        impl<'a> TryFrom<&'a $crate::json::Value> for &'a $name {
            type Error = $crate::Error;
            fn try_from(value: &'a $crate::json::Value) -> $crate::Result<Self> {
                $crate::json::map(value).map(Into::into)
            }
        }
    };
}

/// Defines a borrowed newtype over a JSON array slice (`[Value]`, like `str`)
/// and its owned counterpart over `Vec<Value>` (like `String`). See the
/// module docs.
#[macro_export]
macro_rules! json_array_newtype {
    ($(#[$meta:meta])* $vis:vis struct $slice:ident; $(#[$owned_meta:meta])* $owned_vis:vis struct $owned:ident;) => {
        $(#[$meta])*
        #[derive(Debug, PartialEq)]
        #[repr(transparent)]
        $vis struct $slice(pub [$crate::json::Value]);

        $(#[$owned_meta])*
        #[derive(Clone, Debug, Default, PartialEq)]
        $owned_vis struct $owned(pub Vec<$crate::json::Value>);

        impl std::ops::Deref for $slice {
            type Target = [$crate::json::Value];
            fn deref(&self) -> &[$crate::json::Value] {
                &self.0
            }
        }
        impl std::ops::DerefMut for $slice {
            fn deref_mut(&mut self) -> &mut [$crate::json::Value] {
                &mut self.0
            }
        }
        impl<'a> From<&'a [$crate::json::Value]> for &'a $slice {
            fn from(value: &'a [$crate::json::Value]) -> Self {
                // SAFETY: `$slice` is `#[repr(transparent)]` over `[Value]`.
                unsafe { &*(value as *const [$crate::json::Value] as *const $slice) }
            }
        }
        impl<'a, const N: usize> From<&'a [$crate::json::Value; N]> for &'a $slice {
            fn from(value: &'a [$crate::json::Value; N]) -> Self {
                value.as_slice().into()
            }
        }
        impl<'a> From<&'a Vec<$crate::json::Value>> for &'a $slice {
            fn from(value: &'a Vec<$crate::json::Value>) -> Self {
                value.as_slice().into()
            }
        }
        impl<'a> TryFrom<&'a $crate::json::Value> for &'a $slice {
            type Error = $crate::Error;
            fn try_from(value: &'a $crate::json::Value) -> $crate::Result<Self> {
                $crate::json::list(value).map(Into::into)
            }
        }
        impl Default for &$slice {
            fn default() -> Self {
                let empty: &[$crate::json::Value] = &[];
                empty.into()
            }
        }
        impl<'a> IntoIterator for &'a $slice {
            type Item = &'a $crate::json::Value;
            type IntoIter = std::slice::Iter<'a, $crate::json::Value>;
            fn into_iter(self) -> Self::IntoIter {
                self.0.iter()
            }
        }
        impl ToOwned for $slice {
            type Owned = $owned;
            fn to_owned(&self) -> $owned {
                $owned(self.0.to_vec())
            }
        }

        impl std::ops::Deref for $owned {
            type Target = $slice;
            fn deref(&self) -> &$slice {
                self.0.as_slice().into()
            }
        }
        impl std::ops::DerefMut for $owned {
            fn deref_mut(&mut self) -> &mut $slice {
                let slice: &mut [$crate::json::Value] = self.0.as_mut_slice();
                // SAFETY: `$slice` is `#[repr(transparent)]` over `[Value]`.
                unsafe { &mut *(slice as *mut [$crate::json::Value] as *mut $slice) }
            }
        }
        impl std::borrow::Borrow<$slice> for $owned {
            fn borrow(&self) -> &$slice {
                self
            }
        }
        impl AsRef<$slice> for $owned {
            fn as_ref(&self) -> &$slice {
                self
            }
        }
        impl From<Vec<$crate::json::Value>> for $owned {
            fn from(value: Vec<$crate::json::Value>) -> Self {
                $owned(value)
            }
        }
        impl From<$owned> for Vec<$crate::json::Value> {
            fn from(value: $owned) -> Vec<$crate::json::Value> {
                value.0
            }
        }
        impl From<$owned> for $crate::json::Value {
            fn from(value: $owned) -> $crate::json::Value {
                $crate::json::Value::Array(value.0)
            }
        }
        impl TryFrom<$crate::json::Value> for $owned {
            type Error = $crate::Error;
            fn try_from(value: $crate::json::Value) -> $crate::Result<Self> {
                match value {
                    $crate::json::Value::Array(list) => Ok($owned(list)),
                    _ => Err($crate::Error::illegal_argument("Expected JSON array")),
                }
            }
        }
        impl FromIterator<$crate::json::Value> for $owned {
            fn from_iter<I: IntoIterator<Item = $crate::json::Value>>(iter: I) -> Self {
                $owned(iter.into_iter().collect())
            }
        }
        impl Extend<$crate::json::Value> for $owned {
            fn extend<I: IntoIterator<Item = $crate::json::Value>>(&mut self, iter: I) {
                self.0.extend(iter)
            }
        }
        impl IntoIterator for $owned {
            type Item = $crate::json::Value;
            type IntoIter = std::vec::IntoIter<$crate::json::Value>;
            fn into_iter(self) -> Self::IntoIter {
                self.0.into_iter()
            }
        }
        impl<'a> IntoIterator for &'a $owned {
            type Item = &'a $crate::json::Value;
            type IntoIter = std::slice::Iter<'a, $crate::json::Value>;
            fn into_iter(self) -> Self::IntoIter {
                self.0.iter()
            }
        }
    };
}

/// Defines a newtype over any JSON value. See the module docs.
#[macro_export]
macro_rules! json_value_newtype {
    ($(#[$meta:meta])* $vis:vis struct $name:ident;) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq)]
        #[repr(transparent)]
        $vis struct $name(pub $crate::json::Value);

        impl std::ops::Deref for $name {
            type Target = $crate::json::Value;
            fn deref(&self) -> &$crate::json::Value {
                &self.0
            }
        }
        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut $crate::json::Value {
                &mut self.0
            }
        }
        impl From<$crate::json::Value> for $name {
            fn from(value: $crate::json::Value) -> Self {
                $name(value)
            }
        }
        impl From<$crate::json::Map> for $name {
            fn from(value: $crate::json::Map) -> Self {
                $name($crate::json::Value::Object(value))
            }
        }
        impl From<$name> for $crate::json::Value {
            fn from(value: $name) -> $crate::json::Value {
                value.0
            }
        }
        impl<'a> From<&'a $crate::json::Value> for &'a $name {
            fn from(value: &'a $crate::json::Value) -> Self {
                // SAFETY: `$name` is `#[repr(transparent)]` over `Value`.
                unsafe { &*(value as *const $crate::json::Value as *const $name) }
            }
        }
    };
}

json_object_newtype! {
    /// One journal operation: a single action (`stroke.add`, `object.update`,
    /// …) or a `{"kind":"batch","actions":[…]}` that applies atomically.
    pub struct Operation;
}

json_array_newtype! {
    /// A borrowed list of operations (JSON objects).
    pub struct Operations;
    /// An owned list of operations, as built by the drawing actions.
    pub struct OperationList;
}

json_object_newtype! {
    /// A replica state: the journal's `ops`/`pending`/`history` lists,
    /// selection leases, checkpoints and the cached `effectiveModel`.
    pub struct ReplicaState;
}

json_object_newtype! {
    /// `Model.json()`: the serialized document model.
    pub struct ModelJson;
}

json_object_newtype! {
    /// A layer, stroke or object as stored in the model (the operation that
    /// last set it: `id`, `layer`, `page`, geometry and style fields).
    pub struct EntityContent;
}

json_object_newtype! {
    /// A reference to one stroke or object: `{"kind":"stroke"|"object","id":…}`,
    /// sometimes with extra fields such as `page`.
    pub struct EntityRef;
}

json_array_newtype! {
    /// A borrowed list of entity references.
    pub struct EntityRefs;
    /// An owned list of entity references.
    pub struct EntityRefList;
}

json_value_newtype! {
    /// An entity's `page` exactly as its JSON holds it. Selection lease keys
    /// render it with Java's number formatting (`2` and `2.0` differ).
    pub struct EntityPage;
}

json_object_newtype! {
    /// The structured input of a drawing or document action (the CLI's `--input`
    /// JSON or the app's equivalent).
    pub struct ActionInput;
}

json_array_newtype! {
    /// A borrowed list of conflict records.
    pub struct Conflicts;
    /// An owned list of conflict records (`Model.visibleConflicts`,
    /// `Collaboration.conflicts`).
    pub struct ConflictList;
}

/// Shared content of one stroke or object in the model. Clones share the
/// content; edits copy it first (`Arc::make_mut`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entity(pub std::sync::Arc<EntityContent>);

impl Entity {
    /// Whether both handles share the same content allocation.
    pub fn ptr_eq(&self, other: &Entity) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
    /// Mutable content, copied first when shared.
    pub fn make_mut(&mut self) -> &mut EntityContent {
        std::sync::Arc::make_mut(&mut self.0)
    }
}

impl std::ops::Deref for Entity {
    type Target = EntityContent;
    fn deref(&self) -> &EntityContent {
        &self.0
    }
}

impl AsRef<EntityContent> for Entity {
    fn as_ref(&self) -> &EntityContent {
        &self.0
    }
}

impl From<EntityContent> for Entity {
    fn from(value: EntityContent) -> Self {
        Entity(std::sync::Arc::new(value))
    }
}

impl From<crate::json::Map> for Entity {
    fn from(value: crate::json::Map) -> Self {
        Entity(std::sync::Arc::new(EntityContent(value)))
    }
}

impl From<std::sync::Arc<crate::json::Map>> for Entity {
    /// Shares `value`'s allocation.
    fn from(value: std::sync::Arc<crate::json::Map>) -> Self {
        // SAFETY: `EntityContent` is `#[repr(transparent)]` over `Map`, so the
        // pointee layout is the same.
        Entity(unsafe { std::sync::Arc::from_raw(std::sync::Arc::into_raw(value) as *const EntityContent) })
    }
}

impl From<Operation> for Entity {
    fn from(value: Operation) -> Self {
        Entity::from(value.0)
    }
}

impl From<std::sync::Arc<EntityContent>> for Entity {
    fn from(value: std::sync::Arc<EntityContent>) -> Self {
        Entity(value)
    }
}
