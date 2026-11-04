//! `AppError`: the one error type of the Android port.
//!
//! The Java app let exceptions of several origins propagate through the same
//! `throws Exception` methods: Android framework exceptions (here
//! [`JavaError`]), core exceptions ([`notemaker_core::Error`], which also
//! carries `IOException`s), and `org.json.JSONException`. `AppError` wraps all
//! of them, keeps the Java `Throwable.toString()` text as its `Display`, and
//! answers Java `catch` questions with [`AppError::is`].
//!
//! Conventions (see docs/design/android-port-plan.md):
//! - `throws Exception` / unchecked exceptions that escape → `AppResult<T>`
//!   propagated with `?`. Errors reaching a native callback are rethrown into
//!   Java by the JNI shims in `lib.rs`, so uncaught failures still crash like
//!   Java did.
//! - The Java exception accessors (`class`, `message`, `is`, `stack_trace`)
//!   and `require` live in `java_port::exception`.
//! - `catch (IllegalArgumentException e)` → `Err(e) if e.is("java.lang.IllegalArgumentException")`.
//! - `e.toString()` → `e.to_string()`; `e.getMessage()` → `e.message()`.

use crate::jx::JavaError;
use crate::org_json::JSONException;
use notemaker_core::error::{Error as CoreError, ErrorKind};
use std::fmt;

#[derive(Clone, Debug)]
pub enum AppError {
    /// A Java throwable or JNI failure (Android framework calls).
    Java(JavaError),
    /// A core/CLI exception (`IllegalArgument`, `Io`, `Http`, …). `std::io::Error`
    /// converts into this variant as `java.io.IOException`.
    Core(CoreError),
    /// `org.json.JSONException`.
    Json(JSONException),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    /// `new RuntimeException(message)`.
    pub fn runtime(message: impl Into<String>) -> AppError {
        AppError::Java(JavaError::new("java.lang.RuntimeException", message))
    }
    /// `new RuntimeException(cause)`: message is `cause.toString()`.
    pub fn runtime_cause(cause: &AppError) -> AppError {
        AppError::Java(JavaError::new("java.lang.RuntimeException", cause.to_string()))
    }
    /// `new RuntimeException(message, cause)` (the cause is only visible in
    /// stack traces in Java; its text is appended to `stack` here).
    pub fn runtime_with(message: impl Into<String>, cause: &AppError) -> AppError {
        let mut e = JavaError::new("java.lang.RuntimeException", message);
        e.stack = format!("Caused by: {cause}");
        AppError::Java(e)
    }
    /// `new IllegalArgumentException(message)` (also `Model.require`).
    pub fn illegal_argument(message: impl Into<String>) -> AppError {
        AppError::Core(CoreError::illegal_argument(message))
    }
    /// `new IllegalStateException(message)`.
    pub fn illegal_state(message: impl Into<String>) -> AppError {
        AppError::Core(CoreError::illegal_state(message))
    }
    /// `new IOException(message)`.
    pub fn io(message: impl Into<String>) -> AppError {
        AppError::Core(CoreError::io(message))
    }
    /// The core error, when this is one (`Endpoint.HttpError` status etc.).
    pub fn core(&self) -> Option<&CoreError> {
        match self {
            AppError::Core(e) => Some(e),
            _ => None,
        }
    }
    pub fn kind(&self) -> Option<ErrorKind> {
        self.core().map(|e| e.kind)
    }
}

impl fmt::Display for AppError {
    /// `Throwable.toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Java(e) => f.write_str(&e.text),
            AppError::Core(e) => write!(f, "{e}"),
            AppError::Json(e) => write!(f, "org.json.JSONException: {}", e.0),
        }
    }
}

impl std::error::Error for AppError {}

impl From<JavaError> for AppError {
    fn from(e: JavaError) -> Self {
        AppError::Java(e)
    }
}
impl From<CoreError> for AppError {
    fn from(e: CoreError) -> Self {
        AppError::Core(e)
    }
}
impl From<JSONException> for AppError {
    fn from(e: JSONException) -> Self {
        AppError::Json(e)
    }
}
impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Core(CoreError::from(e))
    }
}
/// `LocalProxy.UiSession` failures (boxed errors from the CLI crate).
impl From<Box<dyn std::error::Error + Send + Sync>> for AppError {
    fn from(e: Box<dyn std::error::Error + Send + Sync>) -> Self {
        match e.downcast::<AppError>() {
            Ok(a) => *a,
            Err(e) => match e.downcast::<CoreError>() {
                Ok(c) => AppError::Core(*c),
                Err(e) => AppError::runtime(e.to_string()),
            },
        }
    }
}

/// Into a Java exception for `jx::guard` / native callbacks. Java-origin
/// errors keep their throwable; others become an exception of the same class
/// (falling back to `RuntimeException` when the class is not loadable).
impl From<AppError> for JavaError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::Java(j) => j,
            other => {
                let class = other.class().to_string();
                let message = other.message();
                let text = other.to_string();
                JavaError { class, message, text, stack: String::new(), throwable: None }
            }
        }
    }
}

/// Into a core error where a core/CLI signature requires one.
impl From<AppError> for CoreError {
    fn from(e: AppError) -> Self {
        match e {
            AppError::Core(c) => c,
            AppError::Json(j) => CoreError::illegal_argument(j.0),
            AppError::Java(j) => {
                if j.is("java.io.IOException") {
                    CoreError::io(j.message.unwrap_or_default())
                } else if j.is("java.lang.IllegalArgumentException") {
                    CoreError::illegal_argument(j.message.unwrap_or_default())
                } else {
                    CoreError::illegal_state(j.text)
                }
            }
        }
    }
}

pub use crate::java_port::exception::require;
