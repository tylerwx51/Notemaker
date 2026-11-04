//! Java library semantics the CLI relies on (`String.split`,
//! `Integer.parseInt`, monitors, `FileChannel.tryLock`).
//!
//! These mirror general-purpose Java APIs, so their signatures use raw
//! strings and numbers on purpose. The crate root exempts this module from
//! `raw_type_in_signature` (ADR 0030). Put a Java helper here only when more
//! than one module uses it; a helper with a single caller belongs next to
//! that caller as a private function.

pub(crate) mod jutil;
