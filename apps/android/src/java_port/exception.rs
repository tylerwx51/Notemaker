//! Java exception semantics of [`AppError`] and [`JavaError`]: exception
//! class names, `getMessage()`, `catch` tests, stack traces and
//! `Model.require` (the Android counterpart of core's
//! `java_port::exception`).

use crate::error::{AppError, AppResult};
use crate::jx::{env, find_class, JavaError};
use jni::objects::JClass;

impl JavaError {
    pub fn new(class: &str, message: impl Into<String>) -> Self {
        let message = message.into();
        JavaError { class: class.into(), text: format!("{class}: {message}"), message: Some(message), stack: String::new(), throwable: None }
    }
    /// True when the Java throwable is an instance of `class` (binary name,
    /// e.g. `java.io.IOException`), including subclasses.
    pub fn is(&self, class: &str) -> bool {
        match &self.throwable {
            Some(t) => {
                let mut env = env();
                match find_class(&class.replace('.', "/")) {
                    Ok(c) => env.is_instance_of(t.as_obj(), unsafe { &JClass::from_raw(c.as_obj().as_raw()) }).unwrap_or(false),
                    Err(_) => false,
                }
            }
            None => self.class == class,
        }
    }
}
impl AppError {
    /// A Java exception of an arbitrary class (`java.lang.AssertionError`, …).
    pub fn java(class: &str, message: impl Into<String>) -> AppError {
        AppError::Java(JavaError::new(class, message))
    }

    /// Binary Java class name of the exception (`java.io.IOException`).
    pub fn class(&self) -> &str {
        match self {
            AppError::Java(e) => &e.class,
            AppError::Core(e) => e.kind.java_class(),
            AppError::Json(_) => "org.json.JSONException",
        }
    }
    /// `getMessage()` (`None` = Java `null`).
    pub fn message(&self) -> Option<String> {
        match self {
            AppError::Java(e) => e.message.clone(),
            AppError::Core(e) => e.message.clone(),
            AppError::Json(e) => Some(e.0.clone()),
        }
    }
    /// `String.valueOf(e.getMessage())`.
    pub fn message_or_null(&self) -> String {
        self.message().unwrap_or_else(|| "null".into())
    }
    /// Java `catch (Class e)` test, including the superclasses the Java code
    /// caught (`Exception`, `RuntimeException`, `IOException`,
    /// `IllegalArgumentException`, …).
    pub fn is(&self, class: &str) -> bool {
        if class == "java.lang.Throwable" || class == "java.lang.Exception" {
            return true;
        }
        match self {
            AppError::Java(e) => e.is(class),
            AppError::Core(e) => {
                let own = e.kind.java_class();
                own == class
                    || match class {
                        "java.lang.RuntimeException" => !e.kind.is_io(),
                        "java.lang.IllegalArgumentException" => e.kind.is_illegal_argument(),
                        "java.io.IOException" => e.kind.is_io(),
                        _ => false,
                    }
            }
            AppError::Json(_) => class == "org.json.JSONException",
        }
    }
    /// `Log.getStackTraceString(e)` approximation for `FAIL:` reports.
    pub fn stack_trace(&self) -> String {
        match self {
            AppError::Java(e) if !e.stack.is_empty() => {
                if e.stack.starts_with(&e.text) {
                    e.stack.clone()
                } else {
                    format!("{}\n{}", e.text, e.stack)
                }
            }
            other => format!("{other}\n"),
        }
    }
}

/// `Model.require(condition, message)` returning `AppResult`.
pub fn require(condition: bool, message: &str) -> AppResult<()> {
    if condition {
        Ok(())
    } else {
        Err(AppError::illegal_argument(message))
    }
}
