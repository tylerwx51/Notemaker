//! NoteMaker shared core: a one-to-one Rust port of
//! `packages/core/src/com/notemaker/core/*.java`.
//!
//! Module names are the Java class names in snake_case and public functions
//! keep the Java method names in snake_case. See `packages/core/README.md`
//! for the class → module table and signature mappings.

pub mod error;
pub mod ids;
pub mod jfmt;
pub mod jmath;
pub mod json;

pub mod actions;
pub mod checkpoint_point_prefixes;
pub mod collaboration;
pub mod coverage;
pub mod draw_item;
pub mod drawing_actions;
pub mod endpoint;
pub mod geometry;
pub mod gif_asset;
pub mod image_asset;
pub mod ink_geometry;
pub mod journal;
pub mod layer_editor;
pub mod model;
pub mod region_fill;
pub mod render;
pub mod shape_catalog;
pub mod store;
pub mod svg_asset;
pub mod timing;
pub mod tls;

pub use error::{Error, ErrorKind, Result};
pub use json::{Map, Value};
pub use model::Model;
