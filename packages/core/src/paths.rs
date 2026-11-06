//! Filesystem path newtypes named by purpose (ADR 0030).
//!
//! `Path` and `PathBuf` say only that a value is a path. A signature uses a
//! type that says what the path is for, such as a replica directory or a
//! journal file, so two paths cannot be swapped by accident. Several purpose
//! types may wrap a `PathBuf`.

/// Defines a `PathBuf` newtype named by purpose. It has a small set of
/// constructors (`new`, `From<PathBuf>`, `From<&Path>`) and derefs to `Path`,
/// so `std::fs` calls and `.join(..)` work on it directly. Converting a plain
/// path into the newtype is always explicit; `PathBuf::from` unwraps it.
#[macro_export]
macro_rules! path_newtype {
    ($(#[$meta:meta])* $vis:vis struct $name:ident;) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        $vis struct $name(std::path::PathBuf);

        impl $name {
            pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
                $name(path.into())
            }
        }
        impl From<$name> for std::path::PathBuf {
            fn from(path: $name) -> std::path::PathBuf {
                path.0
            }
        }
        impl From<std::path::PathBuf> for $name {
            fn from(path: std::path::PathBuf) -> Self {
                $name(path)
            }
        }
        impl From<&std::path::Path> for $name {
            fn from(path: &std::path::Path) -> Self {
                $name(path.to_path_buf())
            }
        }
        impl std::ops::Deref for $name {
            type Target = std::path::Path;
            fn deref(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl AsRef<std::path::Path> for $name {
            fn as_ref(&self) -> &std::path::Path {
                &self.0
            }
        }
    };
}

path_newtype! {
    /// A directory that holds a replica: its document journals and, for a
    /// local proxy, `proxy.json`.
    pub struct ReplicaDir;
}

path_newtype! {
    /// A document's journal base file (`document-<uuid>.json`). The WAL sits
    /// beside it with a `.journal` suffix.
    pub struct JournalPath;
}
