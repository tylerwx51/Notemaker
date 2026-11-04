//! Java exception semantics of [`Error`]: class names, `getMessage()`,
//! I/O messages and `Model.require`.

use crate::error::{Error, ErrorKind, Result};

impl ErrorKind {
    /// Simple Java class name, useful for diagnostics.
    pub fn java_class(self) -> &'static str {
        match self {
            ErrorKind::IllegalArgument => "java.lang.IllegalArgumentException",
            ErrorKind::IllegalState => "java.lang.IllegalStateException",
            ErrorKind::Io => "java.io.IOException",
            ErrorKind::Http => "com.notemaker.core.Endpoint$HttpError",
            ErrorKind::ClassCast => "java.lang.ClassCastException",
            ErrorKind::NullPointer => "java.lang.NullPointerException",
            ErrorKind::UnsupportedOperation => "java.lang.UnsupportedOperationException",
            ErrorKind::IndexOutOfBounds => "java.lang.IndexOutOfBoundsException",
            ErrorKind::NumberFormat => "java.lang.NumberFormatException",
            ErrorKind::OverlappingFileLock => "java.nio.channels.OverlappingFileLockException",
            ErrorKind::Arithmetic => "java.lang.ArithmeticException",
            ErrorKind::SslHandshake => "javax.net.ssl.SSLHandshakeException",
            ErrorKind::SslPeerUnverified => "javax.net.ssl.SSLPeerUnverifiedException",
        }
    }
}

impl Error {
    /// `getMessage()` with Java's `String.valueOf` rendering of null.
    pub fn message(&self) -> &str {
        self.message.as_deref().unwrap_or("null")
    }
    /// Java `String.valueOf(e.getMessage())`, i.e. `"null"` when absent.
    pub fn message_or_null(&self) -> String {
        self.message().to_string()
    }
}

/// Java-like message for an OS-level I/O failure (`path (reason)` is added by callers).
pub fn io_message(e: &std::io::Error) -> String {
    let text = e.to_string();
    // Rust appends " (os error N)"; Java reports the strerror text only.
    match text.rfind(" (os error ") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}

/// `Model.require(condition, message)`.
#[inline]
pub fn require(condition: bool, message: &str) -> Result<()> {
    if condition { Ok(()) } else { Err(Error::illegal_argument(message)) }
}

/// `Model.require` with a lazily built message (string concatenation in Java).
#[inline]
pub fn require_with(condition: bool, message: impl FnOnce() -> String) -> Result<()> {
    if condition { Ok(()) } else { Err(Error::illegal_argument(message())) }
}
