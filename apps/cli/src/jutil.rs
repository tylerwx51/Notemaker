//! Small Java library semantics the CLI relied on (`String.split`,
//! `URLDecoder`, `Integer.parseInt`, hash-map iteration order, monitors,
//! `FileChannel.tryLock`).

use notemaker_core::{json, Error, ErrorKind, Result};
use std::sync::{Condvar, Mutex};
use std::thread::ThreadId;

/// `s.split(regex_literal)` for a literal separator: trailing empty strings
/// are removed; `""` gives `[""]`.
pub(crate) fn split(s: &str, sep: &str) -> Vec<String> {
    let mut parts: Vec<String> = s.split(sep).map(str::to_string).collect();
    if s.is_empty() {
        return vec![String::new()];
    }
    while parts.len() > 1 && parts.last().is_some_and(|p| p.is_empty()) {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() && !s.is_empty() {
        parts.clear();
    }
    parts
}

/// `URLDecoder.decode(s, "UTF-8")`.
pub(crate) fn url_decode(s: &str) -> Result<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' => {
                while i < bytes.len() && bytes[i] == b'%' {
                    if i + 2 >= bytes.len() {
                        return Err(Error::illegal_argument(
                            "URLDecoder: Incomplete trailing escape (%) pattern",
                        ));
                    }
                    let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                        .ok()
                        .and_then(|h| u8::from_str_radix(h, 16).ok());
                    match hex {
                        Some(b) => out.push(b),
                        None => {
                            return Err(Error::illegal_argument(
                                "URLDecoder: Illegal hex characters in escape (%) pattern",
                            ))
                        }
                    }
                    i += 3;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// `Integer.parseInt(s)`.
pub(crate) fn parse_int(s: &str) -> Result<i32> {
    let error = || Error::number_format(format!("For input string: \"{}\"", s));
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(error());
    }
    s.parse::<i32>().map_err(|_| error())
}

/// Iteration order of a `HashMap`/`ConcurrentHashMap` with these keys.
pub(crate) fn hash_order<'a>(keys: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    json::java_hash_set_order(keys.into_iter().cloned())
}

/// A reentrant monitor (`synchronized(this)`).
#[derive(Default)]
pub(crate) struct Monitor {
    owner: Mutex<(Option<ThreadId>, usize)>,
    released: Condvar,
}

pub(crate) struct MonitorGuard<'a>(&'a Monitor);

impl Monitor {
    pub(crate) fn enter(&self) -> MonitorGuard<'_> {
        let me = std::thread::current().id();
        let mut owner = self.owner.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match owner.0 {
                None => {
                    *owner = (Some(me), 1);
                    break;
                }
                Some(t) if t == me => {
                    owner.1 += 1;
                    break;
                }
                Some(_) => owner = self.released.wait(owner).unwrap_or_else(|e| e.into_inner()),
            }
        }
        MonitorGuard(self)
    }
}

impl Drop for MonitorGuard<'_> {
    fn drop(&mut self) {
        let mut owner = self.0.owner.lock().unwrap_or_else(|e| e.into_inner());
        owner.1 -= 1;
        if owner.1 == 0 {
            owner.0 = None;
            self.0.released.notify_one();
        }
    }
}

/// Canonical lock-file paths this process holds (`OverlappingFileLockException`).
fn held_locks() -> &'static Mutex<std::collections::HashSet<std::path::PathBuf>> {
    static HELD: std::sync::OnceLock<Mutex<std::collections::HashSet<std::path::PathBuf>>> =
        std::sync::OnceLock::new();
    HELD.get_or_init(|| Mutex::new(std::collections::HashSet::new()))
}

/// `FileChannel.tryLock()`: exclusive whole-file `fcntl` record lock.
/// `Ok(None)` when another process (or this one) holds it.
pub(crate) struct FileLock {
    file: std::fs::File,
    key: std::path::PathBuf,
}

impl FileLock {
    pub(crate) fn try_lock(path: &std::path::Path) -> Result<Option<FileLock>> {
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| {
                Error::io(format!(
                    "{} ({})",
                    path.display(),
                    notemaker_core::error::io_message(&e)
                ))
            })?;
        let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut held = held_locks().lock().unwrap_or_else(|e| e.into_inner());
        if held.contains(&key) {
            return Ok(None);
        }
        // SAFETY: plain fcntl on an owned descriptor with a zeroed flock.
        let mut lock: libc::flock = unsafe { std::mem::zeroed() };
        lock.l_type = libc::F_WRLCK as _;
        lock.l_whence = libc::SEEK_SET as _;
        lock.l_start = 0;
        lock.l_len = 0;
        let rc = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &lock) };
        if rc == -1 {
            let e = std::io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(libc::EAGAIN) | Some(libc::EACCES) => Ok(None),
                _ => Err(Error::new(
                    ErrorKind::Io,
                    notemaker_core::error::io_message(&e),
                )),
            };
        }
        held.insert(key.clone());
        Ok(Some(FileLock { file, key }))
    }

    /// `release(); channel.close()`.
    pub(crate) fn release(self) {
        use std::os::unix::io::AsRawFd;
        // SAFETY: unlock the same range on the descriptor we locked.
        let mut lock: libc::flock = unsafe { std::mem::zeroed() };
        lock.l_type = libc::F_UNLCK as _;
        lock.l_whence = libc::SEEK_SET as _;
        unsafe { libc::fcntl(self.file.as_raw_fd(), libc::F_SETLK, &lock) };
        held_locks()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}
