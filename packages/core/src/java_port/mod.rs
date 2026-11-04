//! Java library semantics the port relies on: `Math`/`StrictMath`
//! (`jmath`), number formatting and parsing (`jfmt`), the `Json.java`
//! codec with its `Map`/`List` helpers (`json`) and exception class names,
//! messages and `Model.require` (`exception`), `File.getCanonicalPath`
//! (`file`) and `URLEncoder` (`url`).
//!
//! These mirror general-purpose Java APIs, so their signatures use raw
//! numbers, strings and JSON values on purpose. The crate root exempts this
//! module from `raw_type_in_signature` (ADR 0030). Put a Java helper here
//! only when more than one module uses it; a helper with a single caller
//! belongs next to that caller as a private function.

pub mod exception;
pub mod file;
pub mod jfmt;
pub mod jmath;
pub mod json;
pub mod url;
