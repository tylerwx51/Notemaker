//! Java library semantics the app relies on: Android's `org.json`
//! (`org_json`), `java.util.concurrent` executors, futures and latches
//! (`concurrent`), monitors (`monitor`), `java.net.URI` parsing (`uri`),
//! `Integer.parseInt` (`integer`),
//! `Math.round/min/max(float)` and `Number.floatValue()` (`math`), `android.graphics.RectF` geometry
//! (`rect_f`), exception class names, messages and `Model.require`
//! (`exception`), and the generic JNI value marshalling under `jx` (`jvm`).
//!
//! These mirror general-purpose Java APIs, so their signatures use raw
//! numbers, strings and JSON values on purpose. The crate root exempts this
//! module from `raw_type_in_signature` (ADR 0030). Put a Java helper here
//! only when more than one module uses it; a helper with a single caller
//! belongs next to that caller as a private function.

pub mod concurrent;
pub mod exception;
pub mod integer;
pub mod jvm;
pub mod math;
pub mod monitor;
pub mod org_json;
pub mod rect_f;
pub mod uri;
