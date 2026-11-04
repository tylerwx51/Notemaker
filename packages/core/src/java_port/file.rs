//! `java.io.File` path semantics.

use std::fs;
use std::path::Path;

/// `File.getCanonicalPath()` (works for files that do not exist yet).
pub fn canonical_path(file: &Path) -> String {
    if let Ok(p) = fs::canonicalize(file) {
        return p.to_string_lossy().into_owned();
    }
    let absolute = if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::env::current_dir().map(|d| d.join(file)).unwrap_or_else(|_| file.to_path_buf())
    };
    let mut existing = absolute.clone();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let mut out = fs::canonicalize(&existing).unwrap_or(existing);
    for name in rest.into_iter().rev() {
        if name == ".." {
            out.pop();
        } else if name != "." {
            out.push(name);
        }
    }
    out.to_string_lossy().into_owned()
}
