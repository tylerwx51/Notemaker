//! NoteMaker CLI and embedded local proxy: a one-to-one Rust port of
//! `apps/cli/{src,jvm,android}/com/notemaker/cli/*.java`.
//!
//! Module names are the Java class names in snake_case. The JVM (AWT/jsvg) and
//! Android (`android.graphics`/androidsvg) variants of `Png` and
//! `ItemCoverage` collapse into one implementation over the backend-neutral
//! [`canvas::Canvas`] trait. [`raster::RasterCanvas`] (tiny-skia) is the
//! CLI backend; the Android app supplies a JNI `android.graphics.Canvas`
//! backend. See `apps/cli/RUST.md` for the Java → Rust API table.

/// `Main`: argument parsing, local commands and process entry point.
#[path = "cli_main.rs"]
pub mod main;

mod area;
pub mod canvas;
pub mod item_coverage;
#[cfg_attr(dylint_lib = "notemaker_lints", allow(raw_type_in_signature))]
mod java_port;
use java_port::jutil;
pub mod local_proxy;
pub mod png;
pub mod probe;
pub mod raster;
pub mod typeface;
pub mod ui_hub;

pub use canvas::Canvas;
pub use item_coverage::ItemCoverage;
pub use local_proxy::{LocalProxy, UiSession};
pub use png::{Png, StrokeCommands};
