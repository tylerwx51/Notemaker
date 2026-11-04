//! Error type mirroring the Java exception classes thrown by the core.
//!
//! Every error carries the exception kind that Java would have thrown and the
//! exact Java message, so callers that caught a specific Java exception type
//! can match on [`ErrorKind`] and messages printed to users stay identical.

use std::fmt;

/// Java exception class that the original code would have thrown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    /// `IllegalArgumentException` (also `Model.require` failures).
    IllegalArgument,
    /// `IllegalStateException`.
    IllegalState,
    /// `IOException` and subclasses other than `Endpoint.HttpError`.
    Io,
    /// `Endpoint.HttpError` (an `IOException` with an HTTP status).
    Http,
    /// `ClassCastException` from an unchecked cast of JSON data.
    ClassCast,
    /// `NullPointerException` from unboxing or dereferencing missing JSON data.
    NullPointer,
    /// `UnsupportedOperationException` (default `Render.Surface` methods).
    UnsupportedOperation,
    /// `IndexOutOfBoundsException` / `ArrayIndexOutOfBoundsException`.
    IndexOutOfBounds,
    /// `NumberFormatException`.
    NumberFormat,
    /// `OverlappingFileLockException` (nested journal lock in one process).
    OverlappingFileLock,
    /// `ArithmeticException`.
    Arithmetic,
    /// `javax.net.ssl.SSLHandshakeException` (an `IOException`).
    SslHandshake,
    /// `javax.net.ssl.SSLPeerUnverifiedException` (an `IOException`; Android
    /// hostname verification failures).
    SslPeerUnverified,
}

impl ErrorKind {
    /// True for the kinds that are `IllegalArgumentException` in Java
    /// (`NumberFormatException` is a subclass).
    pub fn is_illegal_argument(self) -> bool {
        matches!(self, ErrorKind::IllegalArgument | ErrorKind::NumberFormat)
    }

    /// True for the kinds that are `IOException` in Java (`HttpError` and the
    /// `javax.net.ssl` exceptions are subclasses).
    pub fn is_io(self) -> bool {
        matches!(self, ErrorKind::Io | ErrorKind::Http | ErrorKind::SslHandshake | ErrorKind::SslPeerUnverified)
    }
}

/// A Java-compatible exception: kind, exact message and optional HTTP status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// `getMessage()`; `None` mirrors a Java exception with a null message.
    pub message: Option<String>,
    /// `Endpoint.HttpError.status` for [`ErrorKind::Http`].
    pub status: Option<crate::net::HttpStatus>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Error {
        Error { kind, message: Some(message.into()), status: None }
    }
    pub fn without_message(kind: ErrorKind) -> Error {
        Error { kind, message: None, status: None }
    }
    pub fn illegal_argument(message: impl Into<String>) -> Error {
        Error::new(ErrorKind::IllegalArgument, message)
    }
    pub fn illegal_state(message: impl Into<String>) -> Error {
        Error::new(ErrorKind::IllegalState, message)
    }
    pub fn io(message: impl Into<String>) -> Error {
        Error::new(ErrorKind::Io, message)
    }
    pub fn http(status: crate::net::HttpStatus, message: impl Into<String>) -> Error {
        Error { kind: ErrorKind::Http, message: Some(message.into()), status: Some(status) }
    }
    pub fn class_cast(message: impl Into<String>) -> Error {
        Error::new(ErrorKind::ClassCast, message)
    }
    pub fn null_pointer() -> Error {
        Error::without_message(ErrorKind::NullPointer)
    }
    pub fn unsupported(message: impl Into<String>) -> Error {
        Error::new(ErrorKind::UnsupportedOperation, message)
    }
    pub fn number_format(message: impl Into<String>) -> Error {
        Error::new(ErrorKind::NumberFormat, message)
    }
}

impl fmt::Display for Error {
    /// Matches Java `Throwable.toString()`: `class: message` or `class`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(m) => write!(f, "{}: {}", self.kind.java_class(), m),
            None => write!(f, "{}", self.kind.java_class()),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::io(io_message(&e))
    }
}

pub use crate::java_port::exception::{io_message, require, require_with};
