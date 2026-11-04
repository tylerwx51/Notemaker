//! Port of `MainActivity.java` (incl. inner `CanvasView`, `ErasePreview`,
//! `PendingObjectHistory`).
//!
//! Instance lifecycle: the Java shim `com.notemaker.MainActivity` forwards
//! `onCreate/onStart/onStop/onDestroy/onActivityResult` through
//! `Native.component`. `component_event("onCreate")` creates the Rust
//! instance, stores `Arc::into_raw` in the shim's public `long handle` field
//! and runs [`MainActivity::on_create`]. Any Rust code holding the Java
//! activity recovers the instance with [`MainActivity::from_java`].
//! `onDestroy` runs [`MainActivity::on_destroy`] and then clears `handle` and
//! drops that reference; workers still holding an `Arc` keep the instance
//! alive until they finish, like Java's GC did.
//!
//! Threading: Java fields were touched from the UI thread, `replica-writer`,
//! `local-state` and test threads, mostly inside `synchronized(lock)`. Every
//! mutable field is a [`Field`]; `synchronized(lock)` is `self.lock.enter()`.
//! Ownership (no reference cycles through Java): children hold
//! `Weak<MainActivity>`; closures registered on long-lived Java objects
//! (listeners, view handles, dialog buttons, the repeating `Handler`
//! runnables) capture `Weak`; one-shot posted runnables and worker tasks
//! capture `Arc`. See docs/design/android-port-plan.md.
//!
//! Port notes (behavior kept, mechanics differ):
//! - `CanvasView` caches `getWidth()/getHeight()` from `onSizeChanged`
//!   (`NativeView` mask `SIZE`), which reports every change of the view
//!   size, so `scale()`/`x()`/`y()` need no JNI call per touch point.
//! - `CanvasView.points` keeps a parallel list of the core JSON values of
//!   its `Float` triples (`point_values`), so `strokeValue()` does not
//!   re-format every coordinate on each frame of live ink.
//! - `localLoop` waits with `LocalProxy::await_change_timeout` and checks the
//!   cooperative interrupt (Rust cannot interrupt `awaitChange`).

#![allow(clippy::too_many_arguments)]

use crate::agent_ui::AgentUi;
use crate::components::ComponentEvent;
use crate::android_canvas::{self, AndroidCanvas, RectFExt};
use crate::android_consts::*;
use crate::animation_player::AnimationPlayer;
use crate::app_replica::AppReplica;
use crate::document_browser::DocumentBrowser;
use crate::editor_ui::EditorUi;
use crate::error::{self, AppError, AppResult};
use crate::field::Field;
use crate::jx::{self, JResult, JThread, JavaError, Local, Obj, Reply, WeakObj};
use crate::java_port::uri;
use crate::monitor::Monitor;
use crate::org_json::{self, JSONArray, JSONObject, JV};
use crate::replica_service;
use crate::tile_ink_cache::{self, TileInkCache};
use crate::ui;
use crate::ui_actions::UiActions;
use indexmap::IndexMap;
use notemaker_cli::canvas::{RectF, Style};
use notemaker_cli::item_coverage;
use notemaker_cli::png::{self, Png};
use notemaker_core::draw_item::DrawItem;
use notemaker_core::layer_editor::LayerEditor;
use notemaker_core::model::{Entity, Model};
use notemaker_core::collaboration::SelectionTransform;
use notemaker_core::store::{self, State, Store};
use notemaker_core::{actions, drawing_actions, geometry, gif_asset, ids, image_asset, jfmt, jmath, json, object, render, svg_asset, timing};
use notemaker_core::json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, OnceLock, Weak};

/// `private static final int SVG_PICK_REQUEST = 4512, IMAGE_PICK_REQUEST = 4513, GIF_PICK_REQUEST = 4514`.
pub const SVG_PICK_REQUEST: i32 = 4512;
pub const IMAGE_PICK_REQUEST: i32 = 4513;
pub const GIF_PICK_REQUEST: i32 = 4514;

/// `java.util.function.Consumer<String>` field type.
pub type StringConsumer = Arc<dyn Fn(&str) + Send + Sync>;
/// `Runnable` field type (Rust-side callbacks, not posted to a `Handler`).
pub type Callback = Arc<dyn Fn() + Send + Sync>;
/// `java.util.function.LongConsumer` field type.
pub type LongConsumer = Arc<dyn Fn(i64) + Send + Sync>;

// ---------------------------------------------------------------------------
// Small Java semantics helpers

/// `NullPointerException` (a Java NPE escaping to the top level).
fn npe() -> AppError {
    AppError::Core(notemaker_core::Error::null_pointer())
}
/// `Math.min(float, float)` (NaN and signed-zero rules).
fn fmin(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && b.is_sign_negative() {
        return b;
    }
    if a <= b {
        a
    } else {
        b
    }
}
/// `Math.max(float, float)` (NaN and signed-zero rules).
fn fmax(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}
/// `Integer.parseInt(s)` (ASCII digits, optional sign, no overflow).
fn parse_java_int(s: &str) -> Option<i32> {
    let (negative, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        value = value * 10 + (b - b'0') as i64;
        if value > i32::MAX as i64 + 1 {
            return None;
        }
    }
    let value = if negative { -value } else { value };
    i32::try_from(value).ok()
}
/// Java `"/+$"` removal (`replaceAll("/+$", "")`) on an already trimmed string.
fn strip_trailing_slashes(s: &str) -> &str {
    s.trim_end_matches('/')
}
/// `Json.map(Json.parse(text))`.
fn parse_map(text: &str) -> AppResult<Map> {
    into_map(json::parse(text)?)
}
/// `Json.map(value)` taking ownership.
fn into_map(value: Value) -> AppResult<Map> {
    match value {
        Value::Object(m) => Ok(m),
        other => Err(json::map(&other).err().map(AppError::from).unwrap_or_else(npe)),
    }
}
/// `new JSONObject(map)` (Android `JSONObject.wrap` of every value).
fn json_object(map: &Map) -> JSONObject {
    let o = JSONObject::new();
    for (k, v) in map {
        // Values parsed from JSON are finite, which is all `put` checks.
        let _ = o.put(k, org_json::wrap(v));
    }
    o
}
/// `((Number) m.getOrDefault(key, d)).longValue()`.
fn long_or(m: &Map, key: &str, d: i64) -> AppResult<i64> {
    match m.get(key) {
        None => Ok(d),
        v => Ok(json::as_long(v)?),
    }
}
/// `((Number) m.getOrDefault(key, d)).intValue()`.
fn int_or(m: &Map, key: &str, d: i32) -> AppResult<i32> {
    match m.get(key) {
        None => Ok(d),
        v => Ok(json::as_int(v)?),
    }
}
/// `((Number) m.getOrDefault(key, d)).doubleValue()`.
fn double_or(m: &Map, key: &str, d: f64) -> AppResult<f64> {
    match m.get(key) {
        None => Ok(d),
        v => Ok(json::as_double(v)?),
    }
}
/// `Json.list(m.getOrDefault(key, Collections.emptyList()))`.
fn list_or(m: &Map, key: &str) -> AppResult<Vec<Value>> {
    match m.get(key) {
        None => Ok(Vec::new()),
        Some(v) => Ok(json::list(v)?.clone()),
    }
}
/// `(String) value` for every element of `Json.list(value)` (Java `null`
/// elements cannot be represented and are skipped).
fn string_list(values: &[Value]) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    for v in values {
        if let Some(s) = json::as_string_opt(Some(v))? {
            out.push(s.to_string());
        }
    }
    Ok(out)
}
/// `(String) m.getOrDefault(key, d)` (a stored `null` reads as `"null"`,
/// which the callers then replace exactly as they replaced Java `null`).
fn string_or(m: &Map, key: &str, d: &str) -> AppResult<String> {
    Ok(json::string_or(m, key, d)?.unwrap_or("null").to_string())
}
/// `"expires"` of a transform preview (`((Number) Json.map(raw).getOrDefault("expires", 0)).longValue()`).
fn expires(raw: &Value) -> AppResult<i64> {
    long_or(json::map(raw)?, "expires", 0)
}
/// `layer.equals(m.get("layer"))`.
fn same_str(m: &Map, key: &str, value: &str) -> bool {
    m.get(key).and_then(Value::as_str) == Some(value)
}
/// `Boolean.FALSE.equals(table.get(key))`.
fn is_false_in(table: &IndexMap<String, bool>, key: &str) -> bool {
    table.get(key) == Some(&false)
}
fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().map(|s| Value::String(s.clone())).collect())
}
/// Arguments for `AppReplica.command`.
fn argv(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}
fn owned(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}
/// `map.put(key, value)` on an `eraseTimings` trace.
fn trace_put(trace: &Option<Arc<Field<IndexMap<String, i64>>>>, key: &str) {
    if let Some(t) = trace {
        let now = jx::nano_time().0;
        t.with_mut(|m| {
            m.insert(key.to_string(), now);
        });
    }
}
fn replica_service_writes() -> &'static crate::concurrent::ExecutorService {
    replica_service::writes()
}
/// `new Png(c)` over a Java `Canvas`.
fn draw_with_png(c: &Obj, f: impl FnOnce(&mut Png<'_>) -> notemaker_core::Result<()>) -> AppResult<()> {
    let mut canvas = AndroidCanvas::new(c.clone());
    let mut png = Png::new(&mut canvas);
    f(&mut png)?;
    Ok(())
}
/// `Paint.Style` object.
fn style(s: Style) -> AppResult<Obj> {
    Ok(android_canvas::style_obj(s)?)
}

// ---------------------------------------------------------------------------
// Canvas coordinate spaces and editor values

/// A point in canvas-view pixels: `MotionEvent.getX/getY` of a touch on the
/// `CanvasView`, before [`CanvasView::to_document`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScreenPoint {
    pub x: f32,
    pub y: f32,
}

/// A rectangle in canvas-view pixels (`View.invalidate(l, t, r, b)`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// The stylus pressure of one touch sample (`MotionEvent.getPressure`);
/// recorded ink clamps it to `0..=1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StylusPressure(pub f32);

/// A document-space point at the canvas view's Java `float` precision
/// (touch positions after [`CanvasView::to_document`], gesture anchors).
/// Converts losslessly into core's `f64` [`geometry::DocPoint`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DocPointF {
    pub x: f32,
    pub y: f32,
}
impl From<DocPointF> for geometry::DocPoint {
    fn from(p: DocPointF) -> geometry::DocPoint {
        geometry::DocPoint { x: p.x as f64, y: p.y as f64 }
    }
}

/// Canvas-view pixels per document unit: [`CanvasView::scale`] fits the
/// 1000×1400 page to the view; multiplied by `zoom` it is the effective
/// scale ([`crate::tile_ink_cache::TileInkCache::density_tier`]).
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct ViewScale(pub f32);

notemaker_core::string_newtype! {
    /// A document's layout as stored in its `document` action:
    /// `"fixed"`, `"pages"` or `"infinite"` (kept as the original string so
    /// an unknown type fails where the shared model rejects it).
    pub struct DocumentType;
}

notemaker_core::string_newtype! {
    /// A page template kind: `ruled`, `grid`, `dots`, `planner` or `blank`.
    pub struct TemplateKind;
}

notemaker_core::string_newtype! {
    /// An `erase` action mode: `stroke`, `partial`, `area` or `page`.
    pub struct EraserMode;
}

/// A `layer <command> <id> [argument]` CLI edit of one layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayerCommand {
    /// `layer hide <id>` (archive the layer).
    Hide,
    /// `layer duplicate <id>`.
    Duplicate,
    /// `layer reorder <id> <position>`.
    Reorder(ids::LayerPosition),
    /// `layer merge <id> <target>`.
    Merge(ids::LayerId),
}

/// Whether [`MainActivity::transform_selection`] only previews the
/// transform or also commits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformCommit {
    Preview,
    Commit,
}

/// `private static final class PendingObjectHistory { int index; }`.
/// Compared by identity (`Arc::ptr_eq`) like the Java `ArrayList.remove(Object)`.
pub struct PendingObjectHistory {
    pub index: Field<i32>,
}
impl PendingObjectHistory {
    fn new(index: i32) -> Arc<PendingObjectHistory> {
        Arc::new(PendingObjectHistory { index: Field::new(index) })
    }
}

/// `static final class ErasePreview`.
pub struct ErasePreview {
    pub layer: String,
    pub page: i32,
    /// `List<android.graphics.Path>` built once with `ItemCoverage.path`
    /// (converted with `android_canvas::path_to_java`).
    pub paths: Vec<Obj>,
    pub hidden: HashSet<String>,
}
impl ErasePreview {
    /// `ErasePreview(Model model, String layer, int page, Map input)`; Java
    /// threw `IllegalArgumentException` for invalid input.
    fn new(model: &Model, layer: &str, page: i32, input: &Map) -> AppResult<ErasePreview> {
        let mut paths = Vec::new();
        let mut hidden = HashSet::new();
        if same_str(input, "mode", "partial") {
            for region in drawing_actions::erase_regions(input)?.iter() {
                paths.push(android_canvas::path_to_java(&item_coverage::path(region, notemaker_core::style::PathEnd::Closed))?);
            }
        } else {
            for raw in drawing_actions::erase(model, input, &item_coverage::FACTORY)? {
                let op = json::map(&raw)?;
                let kind = if same_str(op, "kind", "active") { "stroke" } else { "object" };
                hidden.insert(format!("{}:{}", kind, json::java_string(op.get(kind))));
            }
        }
        Ok(ErasePreview { layer: layer.to_string(), page, paths, hidden })
    }
}

/// A `refreshShared(Map)` argument. Java's embedded proxy returned a
/// `Store.Snapshot` map that carried its materialized model, and
/// `Store.model(state)` took that model instead of parsing `effectiveModel`;
/// [`State`] from `AppReplica::read` plays that role here.
pub trait SharedState {
    fn map(&self) -> &Map;
    /// `Store.model(state)`.
    fn shared_model(&self) -> AppResult<Model>;
}
impl SharedState for Map {
    fn map(&self) -> &Map {
        self
    }
    fn shared_model(&self) -> AppResult<Model> {
        Ok(store::model_of_map(self)?)
    }
}
impl SharedState for State {
    fn map(&self) -> &Map {
        self
    }
    fn shared_model(&self) -> AppResult<Model> {
        Ok(store::model(self)?)
    }
}

/// `public class MainActivity extends Activity`.
pub struct MainActivity {
    /// The Java `com.notemaker.MainActivity` (shim) instance: Java's `this`.
    pub this: Obj,

    pub app_session_id: Field<String>,
    pub device_id: Field<String>,
    pub app_session_started_at: Field<i64>,
    pub ui_actions: Field<Option<Arc<UiActions>>>,
    pub animation_player: Field<Option<Arc<AnimationPlayer>>>,
    pub browser: Field<Option<Arc<DocumentBrowser>>>,
    /// `final Object lock`.
    pub lock: Monitor,
    pub event_count: Field<i32>,
    pub pending_count: Field<i32>,
    pub optimistic: Field<IndexMap<String, JSONObject>>,
    pub saving: Field<i32>,
    pub save_error: Field<Option<String>>,
    /// `Map<String,Map<String,Object>> renderedEntities` (model entity identity).
    pub rendered_entities: Field<HashMap<String, Entity>>,
    pub rendered_document: Field<Option<String>>,
    pub leases: Field<Map>,
    pub transform_previews: Field<Map>,
    pub conflicts: Field<Vec<Value>>,
    pub layers: Field<IndexMap<String, JSONObject>>,
    pub strokes: Field<IndexMap<String, JSONObject>>,
    pub active: Field<HashMap<String, bool>>,
    pub hidden: Field<HashSet<String>>,
    pub undo: Field<Vec<String>>,
    pub redo: Field<Vec<String>>,
    pub pending_object_history: Field<Vec<Arc<PendingObjectHistory>>>,
    pub pending_object_previews: Field<IndexMap<String, Entity>>,
    pub drawn_object_previews: Field<HashSet<String>>,
    pub tiled_object_previews: Field<HashSet<String>>,
    pub coalesced_object_frames: Field<i64>,
    pub object_commit_ack: Field<Option<StringConsumer>>,
    pub before_object_commit_callback: Field<Option<Callback>>,
    pub object_refresh_deferred: Field<bool>,
    pub proxy_url: Field<String>,
    pub proxy_token: Field<String>,
    pub replica: Field<Option<Arc<AppReplica>>>,
    pub document: Field<String>,
    pub host: Field<String>,
    /// Java field `type`.
    pub type_: Field<String>,
    pub selected: Field<String>,
    pub cursor: Field<i64>,
    pub page: Field<i32>,
    pub color: Field<i32>,
    pub width: Field<f32>,
    pub material: Field<i32>,
    pub brush: Field<String>,
    pub erase: Field<bool>,
    pub pan: Field<bool>,
    pub connected: Field<bool>,
    pub document_seen: Field<bool>,
    pub fill_tap_mode: Field<bool>,
    pub eraser_mode: Field<String>,
    pub eraser_radius: Field<f32>,
    pub scribble_to_erase: Field<bool>,
    pub drawing_shape: Field<Option<String>>,
    pub pending_text: Field<Option<String>>,
    pub selection_session: Field<Option<String>>,
    /// `volatile String selectionError`.
    pub selection_error: Field<Option<String>>,
    /// `volatile long selectionGeneration`.
    pub selection_generation: Field<i64>,
    /// `volatile long selectionRequestSequence`.
    pub selection_request_sequence: Field<i64>,
    pub selected_entities: Field<Vec<Map>>,
    pub selection_skipped: Field<i32>,
    pub selecting: Field<bool>,
    pub pending_transform: Field<Option<Map>>,
    pub selection_clipboard: Field<Map>,
    /// `final Handler selectionHandler = new Handler(Looper.getMainLooper())`.
    pub selection_handler: Obj,
    /// Java `Runnable`s posted to `selectionHandler` (kept for `removeCallbacks`).
    pub heartbeat_task: Field<Option<Obj>>,
    pub preview_expiry_task: Field<Option<Obj>>,
    pub preview_dispatch_task: Field<Option<Obj>>,
    pub preview_in_flight: Field<bool>,
    pub transform_commit_queued: Field<bool>,
    pub history_pending: Field<bool>,
    pub freeform_lasso: Field<bool>,
    pub pending_text_size: Field<f32>,
    pub pending_text_bold: Field<bool>,
    pub pending_text_italic: Field<bool>,
    pub pending_text_underline: Field<bool>,
    pub pending_text_family: Field<String>,
    pub pending_text_alignment: Field<String>,
    pub object_fill_color: Field<i32>,
    /// `volatile boolean running = true`.
    pub running: Field<bool>,
    /// `volatile boolean foreground`.
    pub foreground: Field<bool>,
    pub canvas: Field<Option<Arc<CanvasView>>>,
    pub editor: Field<Option<Arc<EditorUi>>>,
    pub agent: Field<Option<Arc<AgentUi>>>,
    pub displayed_connected: Field<bool>,
    pub trace_cold_load: Field<bool>,
    pub trace_cold_draw: Field<bool>,
    pub startup_epoch: Field<i64>,
    pub sync_thread: Field<Option<JThread>>,
    pub local_thread: Field<Option<JThread>>,
    /// `Store store` (shared with `replica.local`).
    pub store: Field<Option<Arc<Store>>>,
    /// `Model model`: replaced on refresh, mutated in place by `apply`
    /// (`Arc::make_mut`); `Model before = model` is an `Arc` clone.
    pub model: Field<Arc<Model>>,
    pub model_generation: Field<i64>,
    pub revision: Field<i64>,
    /// `Map<String,Long> eraseTimings` (installed by `ErasePerformance`).
    pub erase_timings: Field<Option<Arc<Field<IndexMap<String, i64>>>>>,
}

/// Routes the shim's lifecycle events (`Native.component`).
pub fn component_event(event: &ComponentEvent) -> JResult<Reply> {
    let ComponentEvent { component, name, args } = *event;
    let arg = |i: usize| args.get(i).and_then(jx::global);
    match &**name {
        "onCreate" => {
            let this = jx::global(component).expect("activity");
            let app = MainActivity::new(this)?;
            let raw = Arc::into_raw(app.clone()) as i64;
            jset!(component, "com/notemaker/MainActivity", "handle", "J", raw)?;
            app.on_create(arg(0)).map_err(jx::JavaError::from)?;
        }
        "onStart" => MainActivity::from_java(component).on_start().map_err(jx::JavaError::from)?,
        "onStop" => MainActivity::from_java(component).on_stop().map_err(jx::JavaError::from)?,
        "onActivityResult" => {
            let request_code = args.first().map(|a| jx::unbox_int(a)).unwrap_or(0);
            let result_code = args.get(1).map(|a| jx::unbox_int(a)).unwrap_or(0);
            MainActivity::from_java(component).on_activity_result(request_code, result_code, arg(2)).map_err(jx::JavaError::from)?;
        }
        "onDestroy" => {
            let app = MainActivity::from_java(component);
            let result = app.on_destroy();
            // Free the handle after the Java onDestroy logic, as the shim's
            // super.onDestroy() runs after this event returns.
            let raw = jget!(component, "com/notemaker/MainActivity", "handle", "J")?.long();
            jset!(component, "com/notemaker/MainActivity", "handle", "J", 0i64)?;
            if raw != 0 {
                drop(unsafe { Arc::from_raw(raw as *const MainActivity) });
            }
            result.map_err(JavaError::from)?;
        }
        other => return Err(jx::JavaError::new("java.lang.IllegalStateException", format!("Unknown MainActivity event {other}"))),
    }
    Ok(Reply::Void)
}

impl MainActivity {
    /// Field initializers of `MainActivity` (Java constructor + instance
    /// initializers); `onCreate` does the rest.
    pub fn new(this: Obj) -> JResult<Arc<MainActivity>> {
        let selection_handler = crate::ui::main_handler()?;
        let mut clipboard = Map::new();
        clipboard.insert("entities".into(), Value::Array(Vec::new()));
        Ok(Arc::new(MainActivity {
            this,
            app_session_id: Field::new(String::new()),
            device_id: Field::new(String::new()),
            app_session_started_at: Field::new(0),
            ui_actions: Field::new(None),
            animation_player: Field::new(None),
            browser: Field::new(None),
            lock: Monitor::new(),
            event_count: Field::new(0),
            pending_count: Field::new(0),
            optimistic: Field::new(IndexMap::new()),
            saving: Field::new(0),
            save_error: Field::new(None),
            rendered_entities: Field::new(HashMap::new()),
            rendered_document: Field::new(None),
            leases: Field::new(Map::new()),
            transform_previews: Field::new(Map::new()),
            conflicts: Field::new(Vec::new()),
            layers: Field::new(IndexMap::new()),
            strokes: Field::new(IndexMap::new()),
            active: Field::new(HashMap::new()),
            hidden: Field::new(HashSet::new()),
            undo: Field::new(Vec::new()),
            redo: Field::new(Vec::new()),
            pending_object_history: Field::new(Vec::new()),
            pending_object_previews: Field::new(IndexMap::new()),
            drawn_object_previews: Field::new(HashSet::new()),
            tiled_object_previews: Field::new(HashSet::new()),
            coalesced_object_frames: Field::new(0),
            object_commit_ack: Field::new(None),
            before_object_commit_callback: Field::new(None),
            object_refresh_deferred: Field::new(false),
            proxy_url: Field::new(String::new()),
            proxy_token: Field::new(String::new()),
            replica: Field::new(None),
            document: Field::new("notes".into()),
            host: Field::new(String::new()),
            type_: Field::new("fixed".into()),
            selected: Field::new("base".into()),
            cursor: Field::new(0),
            page: Field::new(0),
            color: Field::new(crate::android_consts::COLOR_BLACK),
            width: Field::new(5.0),
            material: Field::new(50),
            brush: Field::new("pen".into()),
            erase: Field::new(false),
            pan: Field::new(false),
            connected: Field::new(false),
            document_seen: Field::new(false),
            fill_tap_mode: Field::new(false),
            eraser_mode: Field::new("stroke".into()),
            eraser_radius: Field::new(12.0),
            scribble_to_erase: Field::new(false),
            drawing_shape: Field::new(None),
            pending_text: Field::new(None),
            selection_session: Field::new(None),
            selection_error: Field::new(None),
            selection_generation: Field::new(0),
            selection_request_sequence: Field::new(0),
            selected_entities: Field::new(Vec::new()),
            selection_skipped: Field::new(0),
            selecting: Field::new(false),
            pending_transform: Field::new(None),
            selection_clipboard: Field::new(clipboard),
            selection_handler,
            heartbeat_task: Field::new(None),
            preview_expiry_task: Field::new(None),
            preview_dispatch_task: Field::new(None),
            preview_in_flight: Field::new(false),
            transform_commit_queued: Field::new(false),
            history_pending: Field::new(false),
            freeform_lasso: Field::new(false),
            pending_text_size: Field::new(24.0),
            pending_text_bold: Field::new(false),
            pending_text_italic: Field::new(false),
            pending_text_underline: Field::new(false),
            pending_text_family: Field::new("sans-serif".into()),
            pending_text_alignment: Field::new("left".into()),
            object_fill_color: Field::new(crate::android_consts::COLOR_TRANSPARENT),
            running: Field::new(true),
            foreground: Field::new(false),
            canvas: Field::new(None),
            editor: Field::new(None),
            agent: Field::new(None),
            displayed_connected: Field::new(false),
            trace_cold_load: Field::new(false),
            trace_cold_draw: Field::new(false),
            startup_epoch: Field::new(0),
            sync_thread: Field::new(None),
            local_thread: Field::new(None),
            store: Field::new(None),
            model: Field::new(Arc::new(Model::new())),
            model_generation: Field::new(0),
            revision: Field::new(-1),
            erase_timings: Field::new(None),
        }))
    }

    /// The Rust instance behind a Java `com.notemaker.MainActivity`
    /// (reads the shim's `handle`). Panics if the activity was not created
    /// or is already destroyed (Java code never used a destroyed activity).
    pub fn from_java(activity: &impl jx::AsRaw) -> Arc<MainActivity> {
        let raw = jget!(*activity, "com/notemaker/MainActivity", "handle", "J").map(|r| r.long()).unwrap_or(0);
        assert!(raw != 0, "MainActivity handle is not set");
        let ptr = raw as *const MainActivity;
        unsafe {
            Arc::increment_strong_count(ptr);
            Arc::from_raw(ptr)
        }
    }

    /// `runOnUiThread(() -> f(this))`. Errors are thrown into Java on the UI
    /// thread (an uncaught exception there crashed the Java app too).
    pub fn run_on_ui_thread(self: &Arc<Self>, f: impl FnOnce(&Arc<MainActivity>) -> AppResult<()> + Send + 'static) {
        let app = self.clone();
        let r = jx::try_once_runnable(move || f(&app).map_err(Into::into));
        if let Err(e) = jcall!(self.this, "android/app/Activity", "runOnUiThread", "(Ljava/lang/Runnable;)V", &r) {
            jx::log::e_err(crate::jx::log::NOTEMAKER, "runOnUiThread failed", &e);
        }
    }
    /// `this.getFilesDir()`.
    pub fn files_dir(&self) -> AppResult<PathBuf> {
        Ok(crate::ui::files_dir(&self.this)?)
    }
    /// The current editor (`editor`, set in `onCreate`).
    pub fn editor(&self) -> Arc<EditorUi> {
        self.editor.get().expect("editor")
    }
    /// The current canvas view (`canvas`, set by `EditorUi.build`).
    pub fn canvas(&self) -> Arc<CanvasView> {
        self.canvas.get().expect("canvas")
    }
    /// The current replica (`replica`, set by `load`).
    pub fn replica(&self) -> Arc<AppReplica> {
        self.replica.get().expect("replica")
    }
    /// `model` (an `Arc` clone: Java's `Model m = model`).
    pub fn model(&self) -> Arc<Model> {
        self.model.get()
    }
    pub fn animation_player(&self) -> Arc<AnimationPlayer> {
        self.animation_player.get().expect("animationPlayer")
    }

    // -----------------------------------------------------------------------
    // Internal helpers (no Java counterpart; inline idioms of the Java code)

    /// `target == replica`.
    fn is_current(&self, target: &Arc<AppReplica>) -> bool {
        self.replica.with(|r| r.as_ref().is_some_and(|r| Arc::ptr_eq(r, target)))
    }
    /// `Toast.makeText(this, text, duration).show()`.
    fn toast(&self, text: &str, duration: crate::android_consts::ToastDuration) -> AppResult<()> {
        Ok(ui::toast(&self.this, &ui::UiText::from(text), duration)?)
    }
    fn saving_add(&self, delta: i32) {
        self.saving.with_mut(|s| *s += delta);
    }
    /// `pending = new PendingObjectHistory(undo.size()); pendingObjectHistory.add(pending);`
    fn add_pending(&self) -> Arc<PendingObjectHistory> {
        let pending = PendingObjectHistory::new(self.undo.with(|u| u.len() as i32));
        self.pending_object_history.with_mut(|l| l.push(pending.clone()));
        pending
    }
    /// `pendingObjectHistory.remove(pending)` (identity).
    fn remove_pending(&self, pending: &Arc<PendingObjectHistory>) {
        self.pending_object_history.with_mut(|l| {
            if let Some(i) = l.iter().position(|p| Arc::ptr_eq(p, pending)) {
                l.remove(i);
            }
        });
    }
    /// `int index=Math.max(0,Math.min(pending.index,undo.size()));undo.add(index,"object-action");`
    fn insert_object_action(&self, pending: &Arc<PendingObjectHistory>) -> i32 {
        let wanted = pending.index.get();
        self.undo.with_mut(|u| {
            let index = 0.max(wanted.min(u.len() as i32));
            u.insert(index as usize, "object-action".into());
            index
        })
    }
    /// `for(PendingObjectHistory later:pendingObjectHistory)if(later.index>=index)later.index++;`
    fn shift_pending(&self, index: i32) {
        self.pending_object_history.with(|l| {
            for later in l {
                if later.index.get() >= index {
                    later.index.with_mut(|i| *i += 1);
                }
            }
        });
    }
    /// `try{refreshShared(snapshot);}catch(Exception e){saveError=e.toString();}`
    fn refresh_or_record<S: SharedState + ?Sized>(self: &Arc<Self>, snapshot: &S) {
        if let Err(e) = self.refresh_shared(snapshot) {
            self.save_error.set(Some(e.to_string()));
        }
    }
    /// The common success path of an object-history command run on the UI
    /// thread: `pendingObjectHistory.remove(pending); index...; undo.add;
    /// later.index++; saveError=null; refreshShared; save(); update();`
    fn finish_object_command(self: &Arc<Self>, pending: &Arc<PendingObjectHistory>, snapshot: &Map) -> AppResult<()> {
        self.remove_pending(pending);
        let index = self.insert_object_action(pending);
        self.shift_pending(index);
        self.save_error.set(None);
        self.refresh_or_record(snapshot);
        self.save();
        self.update()
    }
    /// `selectionHandler.removeCallbacks(task)` for a stored task field.
    fn remove_task(&self, task: &Field<Option<Obj>>) -> AppResult<()> {
        if let Some(t) = task.get() {
            ui::remove_callbacks(&self.selection_handler, &t)?;
        }
        Ok(())
    }
    /// `new AlertDialog.Builder(this)`.
    fn dialog_builder(&self) -> AppResult<Obj> {
        Ok(jnew!("android/app/AlertDialog$Builder", "(Landroid/content/Context;)V", &self.this)?)
    }
    /// A `DialogInterface.OnClickListener` whose body gets the activity (captured weakly) and `which`.
    fn dialog_listener(self: &Arc<Self>, f: impl Fn(&Arc<MainActivity>, i32) -> AppResult<()> + Send + Sync + 'static) -> Obj {
        let weak = Arc::downgrade(self);
        jx::callback(move |_, args| {
            let Some(app) = weak.upgrade() else { return Ok(Reply::Void) };
            let which = args.get(1).map(|a| jx::unbox_int(a)).unwrap_or(0);
            f(&app, which).map_err(JavaError::from)?;
            Ok(Reply::Void)
        })
    }
    /// `new Spinner(this)` with an `ArrayAdapter` of `values` (`simple_spinner_dropdown_item`).
    fn spinner(&self, values: &[&str]) -> AppResult<Obj> {
        let spinner = jnew!("android/widget/Spinner", "(Landroid/content/Context;)V", &self.this)?;
        let items = jx::new_string_array(values)?;
        let adapter = jnew!("android/widget/ArrayAdapter", "(Landroid/content/Context;I[Ljava/lang/Object;)V", &self.this, R_LAYOUT_SIMPLE_SPINNER_DROPDOWN_ITEM, &items)?;
        jcall!(spinner, "android/widget/Spinner", "setAdapter", "(Landroid/widget/SpinnerAdapter;)V", &adapter)?;
        Ok(spinner)
    }
    /// `new CheckBox(this); setText(label); setChecked(checked)`.
    fn check_box(&self, label: &str, checked: bool) -> AppResult<Obj> {
        let b = jnew!("android/widget/CheckBox", "(Landroid/content/Context;)V", &self.this)?;
        ui::set_text(&b, &ui::UiText::from(label))?;
        jcall!(b, "android/widget/CompoundButton", "setChecked", "(Z)V", checked)?;
        Ok(b)
    }
    fn edit_text(&self) -> AppResult<Obj> {
        Ok(jnew!("android/widget/EditText", "(Landroid/content/Context;)V", &self.this)?)
    }
    /// `spinner.getSelectedItem().toString()`.
    fn selected_item(spinner: &Obj) -> AppResult<String> {
        let item = jcall!(spinner, "android/widget/AdapterView", "getSelectedItem", "()Ljava/lang/Object;")?.global().ok_or_else(npe)?;
        Ok(jx::to_string(item.as_obj()).unwrap_or_default())
    }
    fn is_checked(b: &Obj) -> AppResult<bool> {
        Ok(jcall!(b, "android/widget/CompoundButton", "isChecked", "()Z")?.bool())
    }
    /// `getSharedPreferences("settings",0).edit().putString(key, value).apply()`.
    fn put_pref(&self, key: &str, value: &str) -> AppResult<()> {
        let e = ui::edit(&self.this)?;
        ui::put_string(&e, &ui::PrefKey::from(key), &ui::PrefText::from(value))?;
        ui::apply(&e)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Statics

    /// `static JSONObject obj(Object... args)`: key/value pairs. Java wrapped a
    /// `JSONException` (non-finite number) in a `RuntimeException`; this panics.
    pub fn obj(args: &[(&str, JV)]) -> JSONObject {
        let o = JSONObject::new();
        for (k, v) in args {
            if let Err(e) = o.put(k, v.clone()) {
                panic!("{}", AppError::runtime_cause(&AppError::from(e)));
            }
        }
        o
    }
    /// `static String id()`: `UUID.randomUUID().toString()`.
    pub fn id() -> ids::Uuid {
        ids::Uuid::random()
    }
    /// `private static void startupMark(boolean enabled, String phase, long started)`.
    fn startup_mark(enabled: bool, phase: &str, started: i64) {
        if enabled {
            jx::log::i(crate::jx::log::STARTUP, &format!("{}={}", phase, jfmt::double((jx::nano_time().0 - started) as f64 / 1e6)));
        }
    }
    /// `static String normalizeServerAddress(String value) throws Exception`
    /// (`java.net.URI` parsing).
    pub fn normalize_server_address(value: &str) -> AppResult<String> {
        let mut address = strip_trailing_slashes(jfmt::java_trim(value)).to_string();
        if address.is_empty() {
            return Ok(String::new());
        }
        if !(address.starts_with("http://") || address.starts_with("https://")) {
            address = format!("http://{address}");
        }
        let uri = uri::parse(&address).map_err(|_| AppError::java("java.net.URISyntaxException", format!("Invalid URI: {address}")))?;
        if uri.host.is_none() || uri.port > 65535 || uri.port == 0 {
            return Err(AppError::illegal_argument("invalid server address"));
        }
        if uri.port < 0 {
            let authority = uri.authority.clone().unwrap_or_else(|| "null".into());
            address = format!(
                "{}://{}:8765{}{}{}",
                uri.scheme.clone().unwrap_or_else(|| "null".into()),
                authority,
                uri.path.clone().unwrap_or_default(),
                uri.query.as_ref().map(|q| format!("?{q}")).unwrap_or_default(),
                uri.fragment.as_ref().map(|f| format!("#{f}")).unwrap_or_default()
            );
        }
        Ok(strip_trailing_slashes(&address).to_string())
    }
    /// `static String serverAddressForSettings(String value)`.
    pub fn server_address_for_settings(value: Option<&str>) -> String {
        let Some(value) = value.filter(|v| !jfmt::java_trim(v).is_empty()) else { return String::new() };
        if let Ok(uri) = uri::parse(value) {
            if let Some(host) = uri.host {
                return host;
            }
        }
        value.to_string()
    }
    /// `static String serverPortForSettings(String value)`.
    pub fn server_port_for_settings(value: Option<&str>) -> String {
        let Some(value) = value.filter(|v| !jfmt::java_trim(v).is_empty()) else { return String::new() };
        match uri::parse(value) {
            Ok(uri) if uri.port > 0 && uri.port != 8765 => uri.port.to_string(),
            _ => String::new(),
        }
    }
    /// `static String composeServerAddress(String addressValue, String portValue, String currentHost) throws Exception`.
    pub fn compose_server_address(address_value: &str, port_value: &str, current_host: Option<&str>) -> AppResult<String> {
        let mut address = jfmt::java_trim(address_value);
        if address.is_empty() {
            if !jfmt::java_trim(port_value).is_empty() {
                return Err(AppError::illegal_argument("Port requires a server address"));
            }
            return Ok(String::new());
        }
        let mut scheme = if current_host.is_some_and(|h| h.starts_with("https://")) { "https" } else { "http" };
        if let Some(rest) = address.strip_prefix("https://") {
            scheme = "https";
            address = rest;
        } else if let Some(rest) = address.strip_prefix("http://") {
            scheme = "http";
            address = rest;
        }
        let text = format!("http://{}", strip_trailing_slashes(address));
        let parsed = uri::parse(&text).map_err(|_| AppError::java("java.net.URISyntaxException", format!("Invalid URI: {text}")))?;
        if parsed.host.is_none() || parsed.user_info.is_some() || parsed.path.as_ref().is_some_and(|p| !p.is_empty()) || parsed.query.is_some() || parsed.fragment.is_some() {
            return Err(AppError::illegal_argument("Enter only a host or IP address"));
        }
        let mut port = parsed.port;
        let supplied_port = jfmt::java_trim(port_value);
        if !supplied_port.is_empty() {
            port = parse_java_int(supplied_port).ok_or_else(|| AppError::illegal_argument("Invalid port"))?;
        }
        if port < 0 {
            port = 8765;
        }
        if !(1..=65535).contains(&port) {
            return Err(AppError::illegal_argument("Port must be between 1 and 65535"));
        }
        Ok(format!("{}://{}:{}", scheme, parsed.host.unwrap_or_default(), port))
    }

    // -----------------------------------------------------------------------
    // Lifecycle (UI thread)

    /// `public void onCreate(Bundle b)` (after `super.onCreate(b)` in the shim).
    pub fn on_create(self: &Arc<Self>, _b: Option<Obj>) -> AppResult<()> {
        let startup_started = jx::nano_time().0;
        self.app_session_id.set(String::from(ids::Uuid::random()));
        self.app_session_started_at.set(jx::current_time_millis().0);
        let package = jcall!(self.this, "android/content/Context", "getPackageName", "()Ljava/lang/String;")?.string().unwrap_or_default();
        let startup_trace = package == "com.notemaker.latency";
        self.trace_cold_load.set(startup_trace);
        self.startup_epoch.set(startup_started);
        self.trace_cold_draw.set(startup_trace);
        Self::startup_mark(startup_trace, "super_done", startup_started);
        let device_id = ui::pref_string(&self.this, &ui::PrefKey::from("uiDeviceId"), Some(&ui::PrefText::from("")))?.map(String::from).unwrap_or_default();
        self.device_id.set(device_id.clone());
        if device_id.is_empty() {
            let id = String::from(ids::Uuid::random());
            self.device_id.set(id.clone());
            self.put_pref("uiDeviceId", &id)?;
        }
        self.scribble_to_erase.set(ui::pref_bool(&self.this, &ui::PrefKey::from("scribbleToErase"), ui::PrefSwitch::from(false))?);
        self.freeform_lasso.set(ui::pref_bool(&self.this, &ui::PrefKey::from("freeformLasso"), ui::PrefSwitch::from(false))?);
        self.material.set(0.max(100.min(ui::pref_int(&self.this, &ui::PrefKey::from("penMaterial"), ui::PrefNumber(50))?.0)));
        let clipboard = (|| -> AppResult<Option<Map>> {
            let saved = ui::pref_string(&self.this, &ui::PrefKey::from("selectionClipboard"), Some(&ui::PrefText::from("")))?.map(String::from).unwrap_or_default();
            if saved.is_empty() {
                return Ok(None);
            }
            Ok(Some(parse_map(&saved)?))
        })();
        if let Ok(Some(clipboard)) = clipboard {
            self.selection_clipboard.set(clipboard);
        }
        let saved_host = ui::pref_string(&self.this, &ui::PrefKey::from("host"), Some(&ui::PrefText::from(&self.host.get())))?.map(String::from);
        let host = match saved_host {
            Some(h) => Self::normalize_server_address(&h).unwrap_or_default(),
            None => String::new(),
        };
        self.host.set(host);
        self.document.set(ui::pref_string(&self.this, &ui::PrefKey::from("document"), Some(&ui::PrefText::from(&self.document.get())))?.map(String::from).unwrap_or_else(|| "null".into()));
        self.proxy_url.set(ui::pref_string(&self.this, &ui::PrefKey::from("proxyUrl"), Some(&ui::PrefText::from("")))?.map(String::from).unwrap_or_default());
        self.proxy_token.set(ui::pref_string(&self.this, &ui::PrefKey::from("proxyToken"), Some(&ui::PrefText::from("")))?.map(String::from).unwrap_or_default());
        Self::startup_mark(startup_trace, "preferences_done", startup_started);
        self.load(&DocumentType::from("fixed"))?;
        self.trace_cold_load.set(false);
        Self::startup_mark(startup_trace, "load_done", startup_started);
        self.agent.set(Some(AgentUi::new(self)?));
        Self::startup_mark(startup_trace, "agent_created", startup_started);
        let editor = EditorUi::new(self)?;
        self.editor.set(Some(editor.clone()));
        self.animation_player.set(Some(AnimationPlayer::new(self)?));
        let root = editor.build()?;
        Self::startup_mark(startup_trace, "view_built", startup_started);
        jcall!(self.this, "android/app/Activity", "setContentView", "(Landroid/view/View;)V", &root)?;
        Self::startup_mark(startup_trace, "content_set", startup_started);
        if startup_trace {
            // The listener removes itself from a posted runnable; both name the
            // listener and the root weakly (the observer owns the listener).
            let first: Arc<OnceLock<WeakObj>> = Arc::new(OnceLock::new());
            let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let root_ref = WeakObj::new(&root);
            let first_ref = first.clone();
            let listener = jx::callback(move |_, _| {
                if !seen.swap(true, Ordering::SeqCst) {
                    Self::startup_mark(true, "first_draw_start", startup_started);
                    let root_ref = root_ref.clone();
                    let first_ref = first_ref.clone();
                    let Some(root) = root_ref.upgrade() else { return Ok(Reply::Void) };
                    let remove = jx::try_once_runnable(move || {
                        if let (Some(root), Some(listener)) = (root_ref.upgrade(), first_ref.get().and_then(WeakObj::upgrade)) {
                            let observer = jcall!(root, "android/view/View", "getViewTreeObserver", "()Landroid/view/ViewTreeObserver;")?.some();
                            jcall!(observer, "android/view/ViewTreeObserver", "removeOnDrawListener", "(Landroid/view/ViewTreeObserver$OnDrawListener;)V", &listener)?;
                        }
                        Self::startup_mark(true, "post_draw_callback", startup_started);
                        Ok(())
                    });
                    ui::view_post(&root, &remove)?;
                }
                Ok(Reply::Void)
            });
            let _ = first.set(WeakObj::new(&listener));
            let observer = jcall!(root, "android/view/View", "getViewTreeObserver", "()Landroid/view/ViewTreeObserver;")?.some();
            jcall!(observer, "android/view/ViewTreeObserver", "addOnDrawListener", "(Landroid/view/ViewTreeObserver$OnDrawListener;)V", &listener)?;
        }
        let sdk = jsget!("android/os/Build$VERSION", "SDK_INT", "I")?.int();
        if sdk >= 30 {
            let app = self.clone();
            let task = jx::try_once_runnable(move || {
                let window = jcall!(app.this, "android/app/Activity", "getWindow", "()Landroid/view/Window;")?.some();
                if let Some(controller) = jcall!(window, "android/view/Window", "getInsetsController", "()Landroid/view/WindowInsetsController;")?.global() {
                    let appearance = WINDOWINSETSCONTROLLER_APPEARANCE_LIGHT_STATUS_BARS | WINDOWINSETSCONTROLLER_APPEARANCE_LIGHT_NAVIGATION_BARS;
                    jcall!(controller, "android/view/WindowInsetsController", "setSystemBarsAppearance", "(II)V", appearance, appearance)?;
                }
                Ok(())
            });
            ui::view_post(&root, &task)?;
        } else {
            let window = jcall!(self.this, "android/app/Activity", "getWindow", "()Landroid/view/Window;")?.some();
            let decor = jcall!(window, "android/view/Window", "getDecorView", "()Landroid/view/View;")?.some();
            jcall!(decor, "android/view/View", "setSystemUiVisibility", "(I)V", VIEW_SYSTEM_UI_FLAG_LIGHT_STATUS_BAR | VIEW_SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR)?;
        }
        self.update()?;
        Self::startup_mark(startup_trace, "update_done", startup_started);
        self.configure_proxy()?;
        let actions = UiActions::new(self)?;
        self.ui_actions.set(Some(actions.clone()));
        let registered = (|| -> AppResult<()> {
            let local = replica_service::ensure(&self.this)?;
            let replica = self.replica();
            local.set_ui_upstream(if replica.remote() { replica.descriptor.as_ref() } else { None });
            local.register_ui_session(&notemaker_cli::local_proxy::UiSessionId::from(self.app_session_id.get()), actions)?;
            Ok(())
        })();
        if let Err(error) = registered {
            jx::log::e_err(crate::jx::log::NOTEMAKER, "Cannot register app session", &error);
        }
        Self::startup_mark(startup_trace, "configure_done", startup_started);
        Ok(())
    }
    /// `public void onDestroy()` (before `super.onDestroy()` in the shim).
    pub fn on_destroy(self: &Arc<Self>) -> AppResult<()> {
        self.running.set(false);
        if let Some(player) = self.animation_player.get() {
            player.stop()?;
        }
        if let Some(proxy) = replica_service::proxy() {
            proxy.unregister_ui_session(&notemaker_cli::local_proxy::UiSessionId::from(self.app_session_id.get()));
        }
        self.remove_task(&self.heartbeat_task)?;
        self.remove_task(&self.preview_expiry_task)?;
        self.remove_task(&self.preview_dispatch_task)?;
        let departing = self.replica.get();
        if let Some(departing) = departing {
            if self.selection_session.with(Option::is_some) {
                replica_service_writes().execute(move || {
                    let _ = departing.command(crate::app_replica::CliArgs(&["selection", "disconnect"]), None);
                });
            }
        }
        if let Some(replica) = self.replica.get() {
            replica.close();
        }
        if let Some(agent) = self.agent.get() {
            agent.destroy()?;
        }
        if let Some(editor) = self.editor.get() {
            editor.dismiss_popup()?;
        }
        if let Some(t) = self.sync_thread.get() {
            t.interrupt();
        }
        if let Some(t) = self.local_thread.get() {
            t.interrupt();
        }
        Ok(())
    }
    /// `protected void onStart()` (after `super.onStart()`).
    pub fn on_start(self: &Arc<Self>) -> AppResult<()> {
        self.foreground.set(true);
        Ok(())
    }
    /// `protected void onStop()` (before `super.onStop()`).
    pub fn on_stop(self: &Arc<Self>) -> AppResult<()> {
        self.foreground.set(false);
        if let Some(player) = self.animation_player.get() {
            player.pause_all()?;
        }
        if let Some(target) = self.replica.get() {
            if !target.remote() {
                let mut journal = target.local.file.clone().into_os_string();
                journal.push(".journal");
                let length = std::fs::metadata(&journal).map(|m| m.len()).unwrap_or(0);
                if length >= 1024 * 1024 {
                    // Queue after outstanding edits. A hidden editor can spend a few
                    // hundred milliseconds folding its WAL without delaying pen input.
                    let app = self.clone();
                    replica_service_writes().execute(move || {
                        if app.foreground.get() {
                            return;
                        }
                        if let Err(e) = target.command(crate::app_replica::CliArgs(&["checkpoint"]), None) {
                            jx::log::w_err(crate::jx::log::NOTEMAKER, "Background checkpoint failed", &e);
                        }
                    });
                }
            }
        }
        Ok(())
    }
    /// `protected void onActivityResult(int requestCode, int resultCode, Intent data)`.
    fn on_activity_result(self: &Arc<Self>, request_code: i32, result_code: i32, data: Option<Obj>) -> AppResult<()> {
        if (request_code != SVG_PICK_REQUEST && request_code != IMAGE_PICK_REQUEST && request_code != GIF_PICK_REQUEST) || result_code != ACTIVITY_RESULT_OK {
            return Ok(());
        }
        let Some(data) = data else { return Ok(()) };
        let Some(uri) = jcall!(data, "android/content/Intent", "getData", "()Landroid/net/Uri;")?.global() else { return Ok(()) };
        let app = self.clone();
        replica_service_writes().execute(move || {
            if let Err(error) = app.import_picture(request_code, &uri) {
                app.run_on_ui_thread(move |app| app.toast(&format!("Could not import picture: {}", error.message_or_null()), TOAST_LENGTH_LONG));
            }
        });
        Ok(())
    }
    /// The `ReplicaService.writes` body of `onActivityResult`.
    fn import_picture(self: &Arc<Self>, request_code: i32, uri: &Obj) -> AppResult<()> {
        let resolver = jcall!(self.this, "android/content/Context", "getContentResolver", "()Landroid/content/ContentResolver;")?.some();
        let input = jcall!(resolver, "android/content/ContentResolver", "openInputStream", "(Landroid/net/Uri;)Ljava/io/InputStream;", uri)?.global();
        let input = input.ok_or_else(|| AppError::illegal_argument("Could not open image"))?;
        let read = (|| -> AppResult<Vec<u8>> {
            let mut bytes: Vec<u8> = Vec::new();
            let buffer = jx::new_byte_array(&[0u8; 8192])?;
            let max_bytes = if request_code == SVG_PICK_REQUEST {
                svg_asset::MAX_BYTES
            } else if request_code == GIF_PICK_REQUEST {
                gif_asset::MAX_BYTES
            } else {
                image_asset::MAX_BYTES
            };
            loop {
                let count = jcall!(input, "java/io/InputStream", "read", "([B)I", &buffer)?.int();
                if count == -1 {
                    break;
                }
                let count = count.max(0) as usize;
                if bytes.len() + count > max_bytes {
                    return Err(AppError::illegal_argument("Image exceeds the supported size"));
                }
                bytes.extend_from_slice(&jx::byte_array(buffer.as_obj())[..count]);
            }
            Ok(bytes)
        })();
        let closed = jcall!(input, "java/io/Closeable", "close", "()V");
        let raw = read?;
        closed?;
        if request_code == SVG_PICK_REQUEST {
            let source = String::from_utf8_lossy(&raw).into_owned();
            let size = svg_asset::validate(&source)?;
            self.run_on_ui_thread(move |app| app.insert_svg(&source, size.width, size.height));
        } else if request_code == GIF_PICK_REQUEST {
            let uri_data = gif_asset::from_bytes(&raw)?;
            let info = gif_asset::validate_bytes(Some(&raw))?;
            self.run_on_ui_thread(move |app| app.insert_gif(&uri_data, info.width as f64, info.height as f64));
        } else {
            let uri_data = image_asset::from_bytes(&raw)?;
            let size = image_asset::validate(Some(&uri_data))?;
            self.run_on_ui_thread(move |app| app.insert_image(&uri_data, size.width, size.height));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // State and persistence

    /// `File stateFile()`: `store.file`.
    pub fn state_file(&self) -> PathBuf {
        self.store.get().expect("store").file.clone()
    }
    /// `void configureProxy()`: (re)starts `local-state` and routes UI sessions.
    pub fn configure_proxy(self: &Arc<Self>) -> AppResult<()> {
        let target = self.replica();
        let target_type = self.type_.get();
        if self.ui_actions.with(Option::is_some) {
            match replica_service::ensure(&self.this) {
                Ok(proxy) => proxy.set_ui_upstream(if target.remote() { target.descriptor.as_ref() } else { None }),
                Err(error) => jx::log::e_err(crate::jx::log::NOTEMAKER, "Cannot update UI session route", &error),
            }
        }
        if !target.remote() {
            let class = jx::find_class("com/notemaker/ReplicaService")?;
            let intent = jnew!("android/content/Intent", "(Landroid/content/Context;Ljava/lang/Class;)V", &self.this, &class)?;
            jcall!(intent, "android/content/Intent", "putExtra", "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;", "document", self.document.get())?;
            jcall!(intent, "android/content/Intent", "putExtra", "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;", "host", self.host.get())?;
            jcall!(self.this, "android/content/Context", "startService", "(Landroid/content/Intent;)Landroid/content/ComponentName;", &intent)?;
        } else {
            let app = self.clone();
            let target = target.clone();
            replica_service_writes().execute(move || {
                let result = (|| -> AppResult<()> {
                    let initial = target.read()?;
                    if json::as_int(initial.get("eventCount"))? == 0 && json::as_int(initial.get("pendingCount"))? == 0 {
                        let input = Value::Object(object! {"kind" => "document", "type" => target_type.clone()});
                        target.command(crate::app_replica::CliArgs(&["append", "-"]), Some(&input))?;
                    }
                    app.refresh_from(&target)
                })();
                if let Err(e) = result {
                    app.run_on_ui_thread(move |app| {
                        app.save_error.set(Some(e.to_string()));
                        app.update()
                    });
                }
            });
        }
        if let Some(t) = self.local_thread.get() {
            t.interrupt();
        }
        let app = self.clone();
        let thread = JThread::spawn(&crate::jx::ThreadName::from("local-state"), move || app.local_loop(target));
        self.local_thread.set(Some(thread));
        Ok(())
    }
    /// `void persist(List<String> args, Object input, String optimisticId)`:
    /// queues `replica.command(args, input)` on `ReplicaService.writes`.
    fn persist(self: &Arc<Self>, args: &[&str], input: Option<Value>, optimistic_id: Option<String>) {
        let target = self.replica();
        let local_settings = !target.remote() && args.len() == 2 && args[0] == "settings" && optimistic_id.is_none();
        if !local_settings {
            self.saving_add(1);
        }
        let args = owned(args);
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = target.command(crate::app_replica::CliArgs(&argv(&args)), input.as_ref()).and_then(|_| target.read());
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    if !local_settings {
                        app.saving_add(-1);
                    }
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    let had_error = app.save_error.with(Option::is_some);
                    app.save_error.set(None);
                    if let Some(id) = &optimistic_id {
                        app.optimistic.with_mut(|o| {
                            o.shift_remove(id);
                        });
                    }
                    let (old_events, old_pending, old_page, old_cursor) = (app.event_count.get(), app.pending_count.get(), app.page.get(), app.cursor.get());
                    let (old_selected, old_type) = (app.selected.get(), app.type_.get());
                    let (old_deleted, old_connected) = (app.model.with(|m| m.deleted), app.connected.get());
                    let (old_leases, old_previews, old_clipboard) = (app.leases.get(), app.transform_previews.get(), app.selection_clipboard.get());
                    let old_conflicts = app.conflicts.get();
                    let refreshed = (|| -> AppResult<()> {
                        let can_skip = local_settings && !had_error && app.revision.get() >= 0 && app.optimistic.with(IndexMap::is_empty);
                        app.refresh_shared(&snapshot)?;
                        if !can_skip
                            || old_events != app.event_count.get()
                            || old_pending != app.pending_count.get()
                            || old_cursor != app.cursor.get()
                            || old_page != app.page.get()
                            || old_selected != app.selected.get()
                            || old_type != app.type_.get()
                            || old_deleted != app.model.with(|m| m.deleted)
                            || old_connected != app.connected.get()
                            || old_leases != app.leases.get()
                            || old_previews != app.transform_previews.get()
                            || old_clipboard != app.selection_clipboard.get()
                            || old_conflicts != app.conflicts.get()
                        {
                            app.update()?;
                        }
                        Ok(())
                    })();
                    if let Err(e) = refreshed {
                        app.save_error.set(Some(e.to_string()));
                        app.update()?;
                    }
                    Ok(())
                }),
                Err(e) => {
                    jx::log::e_err(crate::jx::log::NOTEMAKER, "Local save failed", &e);
                    app.run_on_ui_thread(move |app| {
                        if !local_settings {
                            app.saving_add(-1);
                        }
                        app.save_error.set(Some(e.to_string()));
                        app.update()?;
                        app.toast(&format!("Could not save changes: {}", e.message_or_null()), TOAST_LENGTH_LONG)
                    });
                }
            }
        });
    }
    /// `void setDrawingTool(String shape)`.
    pub fn set_drawing_tool(self: &Arc<Self>, shape: Option<&ids::ShapeId>) -> AppResult<()> {
        let shape: Option<&str> = shape.map(|s| &**s);
        if shape.is_some() && (self.selecting.get() || self.selection_session.with(Option::is_some) || self.selected_entities.with(|s| !s.is_empty())) {
            self.deselect()?;
        }
        let _sync = self.lock.enter();
        let canvas = self.canvas.get();
        let preview = canvas.as_ref().is_some_and(|c| c.shape_dragging.get());
        self.drawing_shape.set(shape.map(str::to_string));
        self.erase.set(false);
        self.pan.set(false);
        self.fill_tap_mode.set(false);
        if shape != Some("text") {
            self.pending_text.set(None);
        }
        if let Some(c) = &canvas {
            c.shape_dragging.set(false);
        }
        if preview {
            self.update()
        } else {
            self.update_chrome()
        }
    }
    /// `void placeText(String text, float fontSize, float x, float y)`.
    pub fn place_text(self: &Arc<Self>, text: Option<&notemaker_core::style::TextContent>, font_size: notemaker_core::style::FontSize, at: DocPointF) -> AppResult<()> {
        let Some(text) = text else { return Ok(()) };
        let text: &str = text;
        // Callers pass the editor's `float` text size, so this is exact.
        let font_size = font_size.0 as f32;
        let DocPointF { mut x, mut y } = at;
        if jfmt::java_trim(text).is_empty() || !font_size.is_finite() || font_size < 1.0 || font_size > 500.0 || !x.is_finite() || !y.is_finite() {
            return Ok(());
        }
        let mut w = fmax(100.0, json::jlen(text) as f32 * font_size * 0.65f32);
        let mut h = fmax(48.0, font_size * 1.6f32);
        if self.type_.get() != "infinite" {
            x = fmax(0.0, fmin(999.0, x));
            y = fmax(0.0, fmin(1399.0, y));
            w = fmax(1.0, fmin(w, 1000.0 - x));
            h = fmax(1.0, fmin(h, 1400.0 - y));
        }
        let object = self.object_value("text", x, y, w, h, self.color.get(), self.width.get(), COLOR_TRANSPARENT);
        let put = (|| -> Result<(), org_json::JSONException> {
            object.put("text", text)?;
            object.put("fontSize", JV::Double(font_size as f64))?;
            object.put("bold", self.pending_text_bold.get())?;
            object.put("italic", self.pending_text_italic.get())?;
            object.put("underline", self.pending_text_underline.get())?;
            object.put("fontFamily", self.pending_text_family.get())?;
            object.put("alignment", self.pending_text_alignment.get())?;
            Ok(())
        })();
        if let Err(e) = put {
            panic!("{}", AppError::runtime_cause(&AppError::from(e)));
        }
        self.persist_object(&object)
    }
    /// `private JSONObject objectValue(String shape, float x, float y, float w, float h, int outline, float outlineWidth, int fill)`.
    fn object_value(&self, shape: &str, x: f32, y: f32, w: f32, h: f32, outline: i32, outline_width: f32, fill: i32) -> JSONObject {
        let object = Self::obj(&[
            ("kind", JV::from("object")),
            ("object", JV::from(String::from(Self::id()))),
            ("shape", JV::from(shape)),
            ("layer", JV::from(self.selected.get())),
            ("page", JV::Int(self.page.get())),
            ("x", JV::Float(x)),
            ("y", JV::Float(y)),
            ("w", JV::Float(w)),
            ("h", JV::Float(h)),
            ("color", JV::Int(outline)),
            ("width", JV::Float(outline_width)),
            ("fontSize", JV::Int(24)),
            ("text", JV::from("")),
        ]);
        if fill != COLOR_TRANSPARENT {
            if let Err(e) = object.put("fill", JV::Int(fill)) {
                panic!("{}", AppError::runtime_cause(&AppError::from(e)));
            }
        }
        object
    }
    /// `private boolean samePaintedObject(Model before, Map preview, String id)`.
    fn same_painted_object(&self, before: &Model, preview: Option<&Map>, id: &str) -> bool {
        let model = self.model();
        let Some(preview) = preview else { return false };
        if before.objects.contains_key(id)
            || is_false_in(&model.object_active, id)
            || before.type_ != model.type_
            || before.deleted != model.deleted
            || before.layers != model.layers
            || before.strokes != model.strokes
            || before.active != model.active
            || before.layer_active != model.layer_active
        {
            return false;
        }
        let old_objects = &before.objects;
        let mut new_objects = model.objects.clone();
        let Some(committed) = new_objects.shift_remove(id) else { return false };
        if *old_objects != new_objects {
            return false;
        }
        let mut new_active = model.object_active.clone();
        new_active.shift_remove(id);
        if before.object_active != new_active {
            return false;
        }
        let Ok(expected) = actions::drawing(preview, &ids::LayerId::from("base")) else { return false };
        let mut visible: Map = (*committed).clone();
        for key in ["id", "actor", "seq", "at", "action", "session", "bases", "visibility"] {
            visible.shift_remove(key);
        }
        json::equivalent(&Value::Object(expected), &Value::Object(visible))
    }
    /// `void persistObject(JSONObject object)`.
    pub fn persist_object(self: &Arc<Self>, object: &JSONObject) -> AppResult<()> {
        let target = self.replica();
        let preview = parse_map(&object.to_string())?;
        let preview_id = json::java_string(preview.get("object"));
        let pending = {
            let _sync = self.lock.enter();
            let pending = self.add_pending();
            self.saving_add(1);
            if !self.model.with(|m| m.objects.contains_key(&preview_id)) {
                self.pending_object_previews.with_mut(|p| {
                    p.insert(preview_id.clone(), Arc::new(preview.clone()));
                });
                if let Some(c) = self.canvas.get() {
                    c.invalidate()?;
                }
            }
            pending
        };
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = (|| -> AppResult<Option<State>> {
                let input = Value::Object(preview);
                Ok(target.command_snapshot(crate::app_replica::CliArgs(&["object", "put", "snapshot", "-"]), Some(&input))?.snapshot)
            })();
            match result {
                Ok(snapshot) => {
                    if let Some(callback) = app.before_object_commit_callback.get() {
                        callback();
                    }
                    app.run_on_ui_thread(move |app| {
                        app.saving_add(-1);
                        if !app.is_current(&target) {
                            return Ok(());
                        }
                        let displayed_preview = app.pending_object_previews.with_mut(|p| p.shift_remove(&preview_id));
                        let mut displayed = app.drawn_object_previews.with_mut(|d| d.remove(&preview_id));
                        displayed &= app.tiled_object_previews.with_mut(|d| d.remove(&preview_id));
                        let before = app.model();
                        app.remove_pending(&pending);
                        let index = app.insert_object_action(&pending);
                        app.shift_pending(index);
                        app.save_error.set(None);
                        app.redo.with_mut(Vec::clear);
                        if let Some(snapshot) = &snapshot {
                            app.refresh_or_record(snapshot);
                        }
                        let retain_pixels = displayed && !target.remote() && app.save_error.with(Option::is_none) && app.same_painted_object(&before, displayed_preview.as_deref(), &preview_id);
                        app.save();
                        if retain_pixels {
                            app.coalesced_object_frames.with_mut(|n| *n += 1);
                            app.update_chrome()?;
                        } else {
                            app.update()?;
                        }
                        if let Some(ack) = app.object_commit_ack.get() {
                            ack(&preview_id);
                        }
                        if snapshot.is_none() || app.object_refresh_deferred.get() {
                            app.object_refresh_deferred.set(false);
                            let app = app.clone();
                            replica_service_writes().execute(move || app.refresh_from_worker(&target));
                        }
                        Ok(())
                    });
                }
                Err(e) => {
                    jx::log::e_err(crate::jx::log::NOTEMAKER, "Object save failed", &e);
                    app.run_on_ui_thread(move |app| {
                        app.saving_add(-1);
                        if !app.is_current(&target) {
                            return Ok(());
                        }
                        app.pending_object_previews.with_mut(|p| {
                            p.shift_remove(&preview_id);
                        });
                        app.drawn_object_previews.with_mut(|d| d.remove(&preview_id));
                        app.tiled_object_previews.with_mut(|d| d.remove(&preview_id));
                        app.remove_pending(&pending);
                        app.save_error.set(Some(e.to_string()));
                        app.update()?;
                        if app.object_refresh_deferred.get() {
                            app.object_refresh_deferred.set(false);
                            let app = app.clone();
                            let target = target.clone();
                            replica_service_writes().execute(move || app.refresh_from_worker(&target));
                        }
                        app.toast(&format!("Could not save object: {}", e.message_or_null()), TOAST_LENGTH_LONG)
                    });
                }
            }
        });
        Ok(())
    }
    /// `ReplicaService.writes.execute(()->refreshFrom(target))`: Java's
    /// `RuntimeException` escaped the executor task; the Rust executor
    /// survives task failures, so it is logged.
    fn refresh_from_worker(self: &Arc<Self>, target: &Arc<AppReplica>) {
        if let Err(e) = self.refresh_from(target) {
            jx::log::e_err(crate::jx::log::NOTEMAKER, "Uncaught failure in executor task", &e);
        }
    }
    /// `boolean objectHistoryPending()` (`synchronized(lock)`).
    pub fn object_history_pending(&self) -> bool {
        let _sync = self.lock.enter();
        self.pending_object_history.with(|l| !l.is_empty())
    }
    /// `Map<String,Object> editorPreferences()`.
    fn editor_preferences(&self) -> Map {
        let hidden = json::java_hash_set_order(self.hidden.with(|h| h.iter().cloned().collect::<Vec<_>>()));
        object! {
            "hidden" => strings(&hidden),
            "undo" => self.undo.with(|u| strings(u)),
            "redo" => self.redo.with(|r| strings(r)),
            "selected" => self.selected.get(),
            "page" => self.page.get(),
            "clipboard" => self.selection_clipboard.get(),
        }
    }
    /// `void save()`.
    pub fn save(self: &Arc<Self>) {
        if self.replica().remote() && self.revision.get() < 0 {
            return;
        }
        self.persist(&["settings", "-"], Some(Value::Object(self.editor_preferences())), None);
    }
    /// `void refreshShared(Map<String,Object> state) throws Exception` (UI thread).
    pub fn refresh_shared<S: SharedState + ?Sized>(self: &Arc<Self>, shared: &S) -> AppResult<()> {
        let state: &Map = shared.map();
        let next = json::as_long(state.get("revision"))?;
        if next < self.revision.get() {
            return Ok(());
        }
        if self.replica().remote() && self.revision.get() < 0 && next >= 0 {
            for key in ["hidden", "undo", "redo"] {
                let values = string_list(&list_or(state, key)?)?;
                match key {
                    "hidden" => self.hidden.with_mut(|h| {
                        h.clear();
                        h.extend(values);
                    }),
                    "undo" => self.undo.set(values),
                    _ => self.redo.set(values),
                }
            }
            self.selected.set(string_or(state, "selected", "base")?);
            self.page.set(int_or(state, "page", 0)?);
        }
        self.revision.set(next);
        let event_count = if state.contains_key("eventCount") { json::as_int(state.get("eventCount"))? } else { json::list(state.get("events").unwrap_or(&Value::Null))?.len() as i32 };
        self.event_count.set(event_count);
        let pending_count = if state.contains_key("pendingCount") { json::as_int(state.get("pendingCount"))? } else { json::list(state.get("pending").unwrap_or(&Value::Null))?.len() as i32 };
        self.pending_count.set(pending_count);
        self.cursor.set(json::as_long(state.get("cursor"))?);
        if let Some(v) = state.get("leases") {
            self.leases.set(json::map(v)?.clone());
        }
        if let Some(v) = state.get("previews") {
            self.transform_previews.set(json::map(v)?.clone());
        }
        if let Some(v) = state.get("conflicts") {
            self.conflicts.set(json::list(v)?.clone());
        }
        if let Some(Value::Object(shared_clipboard)) = state.get("clipboard") {
            let local = self.selection_clipboard.get();
            let shared_at = long_or(shared_clipboard, "copiedAt", 0)?;
            let local_at = long_or(&local, "copiedAt", 0)?;
            let shared_has_items = !list_or(shared_clipboard, "entities")?.is_empty();
            let local_has_items = !list_or(&local, "entities")?.is_empty();
            if shared_at >= local_at && (shared_has_items || !local_has_items) {
                self.selection_clipboard.set(shared_clipboard.clone());
            }
        }
        self.put_pref("selectionClipboard", &json::write_map(&self.selection_clipboard.get()))?;
        self.schedule_preview_expiry()?;
        let model = Arc::new(shared.shared_model()?);
        self.model.set(model.clone());
        self.model_generation.with_mut(|g| *g += 1);
        self.type_.set(model.type_.clone());
        self.document_seen.set(event_count + pending_count > 0);
        if model.deleted {
            self.optimistic.with_mut(IndexMap::clear);
            self.reset_drawing_gesture()?;
            self.deselect()?;
        }
        let mut ordered_layers: Vec<(&String, &Entity)> = model.layers.iter().collect();
        let order = |e: &Entity| match e.get("order") {
            Some(Value::Number(n)) => json::number_double(n),
            _ => 0.0,
        };
        ordered_layers.sort_by(|a, b| order(a.1).total_cmp(&order(b.1)));
        let next_layers: IndexMap<String, JSONObject> = self.layers.with(|previous| ordered_layers.iter().map(|(k, v)| ((*k).clone(), self.entity("layer:", k, v, previous))).collect());
        let next_strokes: IndexMap<String, JSONObject> = self.strokes.with(|previous| model.strokes.iter().map(|(k, v)| (k.clone(), self.entity("stroke:", k, v, previous))).collect());
        self.layers.set(next_layers);
        self.strokes.set(next_strokes);
        self.active.set(model.active.iter().map(|(k, v)| (k.clone(), *v)).collect());
        let ops: Vec<JSONObject> = self.optimistic.with(|o| o.values().cloned().collect());
        for op in ops {
            self.apply(&op)?;
        }
        let selected = self.selected.get();
        if !self.layers.with(|l| l.contains_key(&selected)) || self.model.with(|m| is_false_in(&m.layer_active, &selected)) {
            self.selected.set("base".into());
            self.deselect()?;
            self.reset_drawing_gesture()?;
        }
        Ok(())
    }
    /// `JSONObject entity(String prefix, Map.Entry<String,Map> entry, Map<String,JSONObject> previous)`.
    fn entity(&self, prefix: &str, key: &str, value: &Entity, previous: &IndexMap<String, JSONObject>) -> JSONObject {
        let old = self.rendered_entities.with_mut(|r| r.insert(format!("{prefix}{key}"), value.clone()));
        match previous.get(key) {
            Some(existing) if old.as_ref().is_some_and(|old| Arc::ptr_eq(old, value) || **value == **old) => existing.clone(),
            _ => json_object(value),
        }
    }
    /// `void load(String newType)` (`synchronized(lock)`). Java threw
    /// `RuntimeException("Cannot read saved document", e)`.
    pub fn load(self: &Arc<Self>, new_type: &DocumentType) -> AppResult<()> {
        let new_type: &str = new_type;
        let _sync = self.lock.enter();
        self.reset_drawing_gesture()?;
        if let Some(c) = self.canvas.get() {
            c.erase_previews.with_mut(IndexMap::clear);
        }
        self.pending_object_history.with_mut(Vec::clear);
        self.pending_object_previews.with_mut(IndexMap::clear);
        self.drawn_object_previews.with_mut(HashSet::clear);
        self.tiled_object_previews.with_mut(HashSet::clear);
        self.object_refresh_deferred.set(false);
        self.history_pending.set(false);
        // Reuse unchanged entity wrappers for a warm reopen, never across documents.
        self.event_count.set(0);
        self.pending_count.set(0);
        self.optimistic.with_mut(IndexMap::clear);
        let document = self.document.get();
        if self.rendered_document.get().as_deref() != Some(document.as_str()) {
            self.rendered_entities.with_mut(HashMap::clear);
        }
        self.cursor.set(0);
        self.revision.set(-1);
        self.hidden.with_mut(HashSet::clear);
        self.undo.with_mut(Vec::clear);
        self.redo.with_mut(Vec::clear);
        self.selected.set("base".into());
        self.page.set(0);
        self.type_.set(new_type.to_string());
        let trace = self.trace_cold_load.get();
        let epoch = self.startup_epoch.get();
        Self::startup_mark(trace, "load_reset", epoch);
        let loaded = (|| -> AppResult<()> {
            if let Some(r) = self.replica.get() {
                r.close();
            }
            let replica = AppReplica::new(self, &ids::DocumentName::from(&document), &self.proxy_url.get(), &self.proxy_token.get())?;
            self.replica.set(Some(replica.clone()));
            self.store.set(Some(replica.local.clone()));
            Self::startup_mark(trace, "replica_created", epoch);
            let state = if replica.remote() {
                State::plain(object! {"effectiveModel" => Model::new().json(), "revision" => -1, "cursor" => 0, "eventCount" => 0, "pendingCount" => 0})
            } else {
                replica.read()?
            };
            Self::startup_mark(trace, "snapshot_read", epoch);
            let hidden = string_list(&list_or(&state, "hidden")?)?;
            self.hidden.with_mut(|h| h.extend(hidden));
            let undo = string_list(&list_or(&state, "undo")?)?;
            self.undo.with_mut(|u| u.extend(undo));
            let redo = string_list(&list_or(&state, "redo")?)?;
            self.redo.with_mut(|r| r.extend(redo));
            self.selected.set(string_or(&state, "selected", "base")?);
            self.page.set(int_or(&state, "page", 0)?);
            Self::startup_mark(trace, "saved_preferences_restored", epoch);
            self.refresh_shared(&state)?;
            self.rendered_document.set(Some(document.clone()));
            Self::startup_mark(trace, "shared_state_refreshed", epoch);
            if replica.remote() {
                self.type_.set(new_type.to_string());
            }
            if !replica.remote() && self.event_count.get() == 0 && self.pending_count.get() == 0 {
                self.enqueue(&Self::obj(&[("kind", JV::from("document")), ("type", JV::from(new_type))]))?;
            }
            Self::startup_mark(trace, "load_complete", epoch);
            Ok(())
        })();
        loaded.map_err(|e| AppError::runtime_with("Cannot read saved document", &e))
    }
    /// `void apply(JSONObject o)`.
    pub fn apply(&self, o: &JSONObject) -> AppResult<()> {
        let action = parse_map(&o.to_string())?;
        self.apply_with(o, &action)
    }
    /// `void apply(JSONObject o, Map<String,Object> action)`.
    fn apply_with(&self, o: &JSONObject, action: &Map) -> AppResult<()> {
        let kind = o.opt_string("kind");
        if kind == "batch" {
            let a = o.opt_json_array("actions").ok_or_else(npe)?;
            for i in 0..a.length() {
                self.apply(&a.opt_json_object(i).ok_or_else(npe)?)?;
            }
            return Ok(());
        }
        self.model.with_mut(|m| Arc::make_mut(m).apply(action))?;
        self.model_generation.with_mut(|g| *g += 1);
        if kind == "document" && !self.document_seen.get() {
            self.type_.set(o.opt_string_or("type", "fixed"));
            self.document_seen.set(true);
        }
        if kind == "layer" {
            self.layers.with_mut(|l| {
                l.insert(o.opt_string("layer"), o.clone());
            });
        }
        if kind == "stroke" {
            self.strokes.with_mut(|s| {
                s.insert(o.opt_string("stroke"), o.clone());
            });
        }
        if kind == "active" {
            self.active.with_mut(|a| {
                a.insert(o.opt_string("stroke"), o.opt_boolean("active"));
            });
        }
        Ok(())
    }
    /// `void enqueue(JSONObject o)`.
    pub fn enqueue(self: &Arc<Self>, o: &JSONObject) -> AppResult<()> {
        let action = parse_map(&o.to_string())?;
        self.enqueue_with(o, &action)
    }
    /// `void enqueue(JSONObject o, Map<String,Object> action)`.
    fn enqueue_with(self: &Arc<Self>, o: &JSONObject, action: &Map) -> AppResult<()> {
        let token = String::from(Self::id());
        self.optimistic.with_mut(|m| {
            m.insert(token.clone(), o.clone());
        });
        self.apply_with(o, action)?;
        self.persist(&["append", "-"], Some(Value::Object(action.clone())), Some(token));
        Ok(())
    }
    /// `void enqueueFinalStroke(Map<String,Object> action)`.
    fn enqueue_final_stroke(self: &Arc<Self>, action: &Map) -> AppResult<()> {
        let object = json_object(action);
        let token = String::from(Self::id());
        self.optimistic.with_mut(|m| {
            m.insert(token.clone(), object.clone());
        });
        self.apply_with(&object, action)?;
        let input = object! {"operation" => action.clone(), "settings" => self.editor_preferences()};
        self.persist(&["append", "settings", "-"], Some(Value::Object(input)), Some(token));
        Ok(())
    }
    /// `void update()` (`synchronized(lock)`): chrome + canvas invalidate.
    pub fn update(&self) -> AppResult<()> {
        let _sync = self.lock.enter();
        let Some(editor) = self.editor.get() else { return Ok(()) };
        self.displayed_connected.set(self.connected.get());
        editor.refresh()?;
        self.canvas.get().ok_or_else(npe)?.invalidate()
    }
    /// `void updateChrome()` (`synchronized(lock)`).
    pub fn update_chrome(&self) -> AppResult<()> {
        let _sync = self.lock.enter();
        let Some(editor) = self.editor.get() else { return Ok(()) };
        self.displayed_connected.set(self.connected.get());
        editor.refresh()
    }

    // -----------------------------------------------------------------------
    // Dialogs and documents (UI thread)

    /// `void documentDialog()`.
    pub fn document_dialog(self: &Arc<Self>) -> AppResult<()> {
        let editor = self.editor();
        let fields = ui::linear_layout(&self.this, crate::android_consts::Orientation::Vertical)?;
        ui::set_padding(&fields, ui::PxInsets { left: ui::dp(&editor.context(), ui::Dp(24.0))?.0, top: ui::dp(&editor.context(), ui::Dp(8.0))?.0, right: ui::dp(&editor.context(), ui::Dp(24.0))?.0, bottom: ui::dp(&editor.context(), ui::Dp(8.0))?.0 })?;
        let name = self.edit_text()?;
        jcall!(name, "android/widget/TextView", "setSingleLine", "()V")?;
        ui::set_text(&name, &ui::UiText::from(&self.document.get()))?;
        jcall!(name, "android/widget/TextView", "setHint", "(Ljava/lang/CharSequence;)V", "Document name (same on both clients)")?;
        ui::add_view(&fields, &name)?;
        let types = self.spinner(&["Fixed page", "Multiple pages", "Infinite canvas"])?;
        let type_ = self.type_.get();
        jcall!(types, "android/widget/Spinner", "setSelection", "(I)V", if type_ == "pages" { 1 } else if type_ == "infinite" { 2 } else { 0 })?;
        ui::add_view(&fields, &types)?;
        let (name_ref, types_ref) = (WeakObj::new(&name), WeakObj::new(&types));
        let open = self.dialog_listener(move |app, _| {
            let (name, types) = (name_ref.get(), types_ref.get());
            let next = jfmt::java_trim(&String::from(ui::text_of(&name)?)).to_string();
            if next.is_empty() {
                return app.toast("Enter a document name", TOAST_LENGTH_LONG);
            }
            app.deselect()?;
            {
                let _sync = app.lock.enter();
                app.save();
                app.document.set(next);
                app.connected.set(false);
                let position = jcall!(types, "android/widget/AdapterView", "getSelectedItemPosition", "()I")?.int();
                let new_type = ["fixed", "pages", "infinite"].get(position as usize).ok_or_else(|| AppError::java("java.lang.ArrayIndexOutOfBoundsException", format!("Index {position} out of bounds for length 3")))?;
                app.load(&DocumentType::from(*new_type))?;
                let canvas = app.canvas();
                canvas.zoom.set(1.0);
                canvas.offset_x.set(0.0);
                canvas.offset_y.set(0.0);
            }
            app.put_pref("document", &app.document.get())?;
            app.configure_proxy()?;
            app.update()
        });
        let b = self.dialog_builder()?;
        jcall!(b, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", "Open or create document")?;
        jcall!(b, "android/app/AlertDialog$Builder", "setMessage", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", "Type is used when creating a new document. Existing documents retain their type.")?;
        jcall!(b, "android/app/AlertDialog$Builder", "setView", "(Landroid/view/View;)Landroid/app/AlertDialog$Builder;", &fields)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Open", &open)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Cancel", jx::NULL)?;
        jcall!(b, "android/app/AlertDialog$Builder", "show", "()Landroid/app/AlertDialog;")?;
        Ok(())
    }
    /// `void saveNetworkSettings(String serverValue, String portValue, String proxyValue, String tokenValue)`.
    pub fn save_network_settings(self: &Arc<Self>, server_value: &str, port_value: &str, proxy_value: &str, token_value: &str) -> AppResult<()> {
        let next_host = match Self::compose_server_address(server_value, port_value, Some(&self.host.get())) {
            Ok(h) => h,
            Err(_) => return self.toast("Enter a server address and a port from 1 to 65535, or leave the port blank for 8765", TOAST_LENGTH_LONG),
        };
        let next_proxy = strip_trailing_slashes(jfmt::java_trim(proxy_value)).to_string();
        let next_token = jfmt::java_trim(token_value).to_string();
        if !next_proxy.is_empty() && (!(next_proxy.starts_with("http://") || next_proxy.starts_with("https://")) || next_token.is_empty()) {
            return self.toast("Enter a proxy HTTP(S) URL and token, or leave both blank", TOAST_LENGTH_LONG);
        }
        let changed = self.host.get() != next_host || self.proxy_url.get() != next_proxy || self.proxy_token.get() != next_token;
        if !changed {
            return self.editor().dismiss_popup();
        }
        self.deselect()?;
        {
            let _sync = self.lock.enter();
            self.save();
            self.host.set(next_host);
            self.proxy_url.set(next_proxy);
            self.proxy_token.set(next_token);
            self.connected.set(false);
            self.load(&DocumentType::from(self.type_.get()))?;
        }
        let e = ui::edit(&self.this)?;
        ui::put_string(&e, &ui::PrefKey::from("host"), &ui::PrefText::from(&self.host.get()))?;
        ui::put_string(&e, &ui::PrefKey::from("proxyUrl"), &ui::PrefText::from(&self.proxy_url.get()))?;
        ui::put_string(&e, &ui::PrefKey::from("proxyToken"), &ui::PrefText::from(&self.proxy_token.get()))?;
        ui::apply(&e)?;
        self.configure_proxy()?;
        self.update()?;
        self.toast("Network settings saved", TOAST_LENGTH_SHORT)
    }
    /// `void openDocument(String name, String documentType)`.
    pub fn open_document(self: &Arc<Self>, name: &ids::DocumentName, document_type: &DocumentType) -> AppResult<()> {
        let name: &str = name;
        self.deselect()?;
        self.animation_player().reset()?;
        {
            let _sync = self.lock.enter();
            self.save();
            self.document.set(name.to_string());
            self.connected.set(false);
            self.load(document_type)?;
            let canvas = self.canvas();
            canvas.zoom.set(1.0);
            canvas.offset_x.set(0.0);
            canvas.offset_y.set(0.0);
        }
        self.put_pref("document", &self.document.get())?;
        self.configure_proxy()?;
        self.update()
    }
    /// `void pageDialog()`.
    pub fn page_dialog(self: &Arc<Self>) -> AppResult<()> {
        if self.type_.get() != "pages" {
            return Ok(());
        }
        let field = self.edit_text()?;
        jcall!(field, "android/widget/TextView", "setInputType", "(I)V", 2)?;
        ui::set_text(&field, &ui::UiText::from(&(self.page.get() + 1).to_string()))?;
        let field_ref = WeakObj::new(&field);
        let open = self.dialog_listener(move |app, _| {
            let result = (|| -> AppResult<()> {
                let text = String::from(ui::text_of(&field_ref.get())?);
                let value = parse_java_int(&text).ok_or_else(|| AppError::Core(notemaker_core::Error::number_format(format!("For input string: \"{text}\""))))?;
                let next = 0.max(10000.min(value.wrapping_sub(1)));
                app.deselect()?;
                app.reset_drawing_gesture()?;
                app.page.set(next);
                app.save();
                app.update()
            })();
            let _ = result;
            Ok(())
        });
        let b = self.dialog_builder()?;
        jcall!(b, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", "Go to / create page")?;
        jcall!(b, "android/app/AlertDialog$Builder", "setView", "(Landroid/view/View;)Landroid/app/AlertDialog$Builder;", &field)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Open", &open)?;
        jcall!(b, "android/app/AlertDialog$Builder", "show", "()Landroid/app/AlertDialog;")?;
        Ok(())
    }
    /// `void history(boolean forward)`.
    pub fn history(self: &Arc<Self>, direction: store::HistoryDirection) -> AppResult<()> {
        let forward = direction == store::HistoryDirection::Redo;
        let target = self.replica();
        let entry = {
            let _sync = self.lock.enter();
            let from = if forward { &self.redo } else { &self.undo };
            if self.history_pending.get() || self.pending_object_history.with(|l| !l.is_empty()) || from.with(Vec::is_empty) {
                return Ok(());
            }
            let entry = from.with(|f| f[f.len() - 1].clone());
            self.history_pending.set(true);
            self.saving_add(1);
            entry
        };
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = (|| -> AppResult<State> {
                let snapshot = if entry == "object-action" {
                    target.command_snapshot(crate::app_replica::CliArgs(&[if forward { "redo" } else { "undo" }, "snapshot"]), None)?.snapshot
                } else {
                    let erased = entry.starts_with("erase:");
                    let stroke = if erased { entry[6..].to_string() } else { entry.clone() };
                    let input = Value::Object(object! {"kind" => "active", "stroke" => stroke, "active" => if erased { !forward } else { forward }});
                    target.command_snapshot(crate::app_replica::CliArgs(&["append", "snapshot", "-"]), Some(&input))?.snapshot
                };
                // Java: map(result.get("snapshot")) rejected a missing snapshot.
                match snapshot {
                    Some(s) => Ok(s),
                    None => Ok(State::plain(into_map(Value::Null)?)),
                }
            })();
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    let _sync = app.lock.enter();
                    let (from, to) = if forward { (&app.redo, &app.undo) } else { (&app.undo, &app.redo) };
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.history_pending.set(false);
                    if from.with(|f| f.last() != Some(&entry)) {
                        app.save_error.set(Some("History changed before the operation completed".into()));
                        return app.update();
                    }
                    from.with_mut(|f| f.pop());
                    to.with_mut(|t| t.push(entry));
                    app.save_error.set(None);
                    app.refresh_or_record(&snapshot);
                    app.save();
                    app.update()
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    let _sync = app.lock.enter();
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.history_pending.set(false);
                    app.save_error.set(Some(error.to_string()));
                    app.update()?;
                    app.toast(&format!("Could not {}: {}", if forward { "redo" } else { "undo" }, error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
        Ok(())
    }
    /// `void refreshFrom(AppReplica target)` (worker threads; posts to UI).
    /// Java wrapped read failures in `RuntimeException`.
    pub fn refresh_from(self: &Arc<Self>, target: &Arc<AppReplica>) -> AppResult<()> {
        let snapshot = target.read().map_err(|e| AppError::runtime_cause(&e))?;
        let target = target.clone();
        self.run_on_ui_thread(move |app| {
            if !app.is_current(&target) {
                return Ok(());
            }
            if let Err(e) = app.apply_refresh(&target, &snapshot) {
                jx::log::e_err(crate::jx::log::NOTEMAKER, "Refresh failed", &e);
            }
            Ok(())
        });
        Ok(())
    }
    /// The `runOnUiThread` body of `refreshFrom` (inside its `try`).
    fn apply_refresh<S: SharedState + ?Sized>(self: &Arc<Self>, target: &Arc<AppReplica>, shared: &S) -> AppResult<()> {
        let snapshot: &Map = shared.map();
        let revision = json::as_long(snapshot.get("revision"))?;
        if !target.remote() && self.pending_object_previews.with(|p| !p.is_empty()) && revision > self.revision.get() {
            let incoming = shared.shared_model()?;
            let deferred = self.pending_object_previews.with(|p| p.keys().any(|id| incoming.objects.contains_key(id)));
            if deferred {
                // The save callback owns the preview-to-model handoff. A listener
                // refresh here would invalidate its already painted frame.
                self.object_refresh_deferred.set(true);
                return Ok(());
            }
        }
        let was_connected = self.connected.get();
        let document = self.document.get();
        let connected = target.remote() || replica_service::proxy().is_some_and(|p| p.connected.lock().unwrap_or_else(|e| e.into_inner()).get(&document) == Some(&true));
        self.connected.set(connected);
        if json::as_long(snapshot.get("revision"))? > self.revision.get() {
            self.refresh_shared(shared)?;
            self.update()?;
        } else {
            let next_leases = match snapshot.get("leases") {
                None => Map::new(),
                Some(v) => json::map(v)?.clone(),
            };
            let next_previews = match snapshot.get("previews") {
                None => Map::new(),
                Some(v) => json::map(v)?.clone(),
            };
            let lease_changed = self.leases.with(|l| *l != next_leases);
            let preview_changed = self.transform_previews.with(|p| *p != next_previews);
            if lease_changed || preview_changed {
                self.leases.set(next_leases);
                self.transform_previews.set(next_previews);
                self.schedule_preview_expiry()?;
                if preview_changed {
                    if let Some(c) = self.canvas.get() {
                        c.invalidate()?;
                    }
                }
                self.editor().refresh()?;
            }
            if was_connected != self.connected.get() {
                self.update()?;
            }
        }
        Ok(())
    }
    /// `void localLoop(AppReplica target)`: body of the `local-state` thread.
    pub fn local_loop(self: &Arc<Self>, target: Arc<AppReplica>) {
        let mut observed: i64 = -1;
        let mut backoff: u64 = 500;
        while self.running.get() && self.is_current(&target) {
            let step = jx::frame(|| -> Result<(), Option<AppError>> {
                if target.remote() {
                    let app = self.clone();
                    let listened = target.listen(&|| {
                        // Java let this RuntimeException end `listen`; the Rust
                        // stream reader cannot be unwound from the callback, so the
                        // failure is reported like the loop's catch did.
                        if let Err(e) = app.refresh_from(&target) {
                            jx::log::e_err(crate::jx::log::NOTEMAKER, "Local state refresh failed", &e);
                        }
                    });
                    listened.map_err(Some)?;
                } else {
                    let proxy = replica_service::ensure(&self.this).map_err(Some)?;
                    observed = loop {
                        if JThread::current_interrupted() {
                            return Err(None);
                        }
                        let generation = proxy.await_change_timeout(observed, std::time::Duration::from_millis(250));
                        if generation != observed {
                            break generation;
                        }
                        if !self.running.get() || !self.is_current(&target) {
                            return Err(None);
                        }
                    };
                    self.refresh_from(&target).map_err(Some)?;
                }
                Ok(())
            });
            match step {
                Ok(()) => backoff = 500,
                // InterruptedException
                Err(None) => return,
                Err(Some(e)) => {
                    jx::log::e_err(crate::jx::log::NOTEMAKER, "Local state refresh failed", &e);
                    let target = target.clone();
                    self.run_on_ui_thread(move |app| {
                        if app.is_current(&target) && app.connected.get() {
                            app.connected.set(false);
                            app.update()?;
                        }
                        Ok(())
                    });
                    if JThread::sleep(jx::DelayMillis(backoff as i64)).is_err() {
                        return;
                    }
                    backoff = 30000.min(backoff * 2);
                }
            }
        }
    }
    /// `public Process startCli(String... arguments) throws IOException`.
    /// The Rust build runs `libnotemaker_cli.so` from `nativeLibraryDir`
    /// (docs/design/rust-port.md, "Android") with the same arguments.
    pub fn start_cli(&self, arguments: crate::app_replica::CliArgs) -> AppResult<std::process::Child> {
        let arguments = arguments.0;
        let mut ui_command = false;
        let mut i = 0;
        while i < arguments.len() {
            if arguments[i].starts_with("--") {
                i += 2;
                continue;
            }
            ui_command = arguments[i] == "ui";
            break;
        }
        let descriptor = if ui_command { self.files_dir()?.join("proxy.json") } else { self.replica().descriptor_file.clone() };
        let info = jcall!(self.this, "android/content/Context", "getApplicationInfo", "()Landroid/content/pm/ApplicationInfo;")?.some();
        let native_dir = jget!(info, "android/content/pm/ApplicationInfo", "nativeLibraryDir", "Ljava/lang/String;")?.string().unwrap_or_default();
        let child = std::process::Command::new(PathBuf::from(native_dir).join("libnotemaker_cli.so"))
            .arg("--proxy-file")
            .arg(&descriptor)
            .arg("--document")
            .arg(self.document.get())
            .args(arguments)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        Ok(child)
    }
    /// `void objectDialog()` (`synchronized(lock)`).
    pub fn object_dialog(self: &Arc<Self>) -> AppResult<()> {
        let _sync = self.lock.enter();
        let model = self.model();
        let page = self.page.get();
        let mut ids: Vec<String> = Vec::new();
        for object in model.objects.values() {
            if int_or(object, "page", 0)? == page {
                ids.push(json::java_string(object.get("object")));
            }
        }
        let mut labels: Vec<String> = Vec::new();
        for id in &ids {
            let o = model.objects.get(id).ok_or_else(npe)?;
            let text = match o.get("text") {
                None => id.clone(),
                v => json::java_string(v),
            };
            labels.push(format!("{} · {}", json::java_string(o.get("shape")), text));
        }
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let items = jx::new_string_array(&labels)?;
        let listener = self.dialog_listener(move |app, index| {
            let id = ids.get(index as usize).ok_or_else(|| AppError::java("java.lang.IndexOutOfBoundsException", format!("Index {index}")))?;
            app.edit_object(&ids::EntityId::from(id))
        });
        let b = self.dialog_builder()?;
        jcall!(b, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", "Edit diagram object")?;
        jcall!(b, "android/app/AlertDialog$Builder", "setItems", "([Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", &items, &listener)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Close", jx::NULL)?;
        jcall!(b, "android/app/AlertDialog$Builder", "show", "()Landroid/app/AlertDialog;")?;
        Ok(())
    }
    /// `new Intent(Intent.ACTION_OPEN_DOCUMENT)` with type and `CATEGORY_OPENABLE`.
    fn picker(&self, mime: &str, extra_types: Option<&[&str]>, request: i32) -> AppResult<()> {
        let picker = jnew!("android/content/Intent", "(Ljava/lang/String;)V", INTENT_ACTION_OPEN_DOCUMENT)?;
        jcall!(picker, "android/content/Intent", "setType", "(Ljava/lang/String;)Landroid/content/Intent;", mime)?;
        if let Some(types) = extra_types {
            let array = jx::new_string_array(types)?;
            jcall!(picker, "android/content/Intent", "putExtra", "(Ljava/lang/String;[Ljava/lang/String;)Landroid/content/Intent;", INTENT_EXTRA_MIME_TYPES, &array)?;
        }
        jcall!(picker, "android/content/Intent", "addCategory", "(Ljava/lang/String;)Landroid/content/Intent;", INTENT_CATEGORY_OPENABLE)?;
        jcall!(self.this, "android/app/Activity", "startActivityForResult", "(Landroid/content/Intent;I)V", &picker, request)?;
        Ok(())
    }
    /// `void choosePicture()`.
    pub fn choose_picture(&self) -> AppResult<()> {
        self.picker("image/png", Some(&["image/png", "image/jpeg"]), IMAGE_PICK_REQUEST)
    }
    /// `void chooseGif()`.
    pub fn choose_gif(&self) -> AppResult<()> {
        self.picker("image/gif", None, GIF_PICK_REQUEST)
    }
    /// `void chooseSvgPicture()`.
    pub fn choose_svg_picture(&self) -> AppResult<()> {
        self.picker("image/svg+xml", None, SVG_PICK_REQUEST)
    }
    /// `void insertTemplateSvg(String source)`.
    pub fn insert_template_svg(self: &Arc<Self>, source: &str) -> AppResult<()> {
        let result = svg_asset::validate(source).map_err(AppError::from).and_then(|_| self.insert_svg_template(source, 1000.0, 1400.0, true));
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.toast(&format!("Template could not be added: {}", error.message_or_null()), TOAST_LENGTH_LONG),
        }
    }
    /// `void insertTemplate(String template)`.
    pub fn insert_template(self: &Arc<Self>, template: &TemplateKind) -> AppResult<()> {
        let template: &str = template;
        if !["ruled", "grid", "dots", "planner", "blank"].contains(&template) {
            return Ok(());
        }
        let target = self.replica();
        let pending = {
            let _sync = self.lock.enter();
            let pending = self.add_pending();
            self.saving_add(1);
            self.update()?;
            pending
        };
        let template_color: i32 = if self.type_.get() == "infinite" { 0xff777777u32 as i32 } else { 0xffc8ccc8u32 as i32 };
        let input = Value::Object(object! {"template" => template, "layer" => self.selected.get(), "page" => self.page.get(), "x" => 0, "y" => 0, "w" => 1000, "h" => 1400, "spacing" => 32, "color" => template_color});
        self.object_command(target, pending, vec![owned(&["template", "add", "-"])], Some(input), "Could not insert template: ");
        Ok(())
    }
    /// Worker part shared by `insertTemplate`, `fillEntity`, `layerCommand`
    /// and `fillAt`: run `commands` (each with `input`), read, then finish
    /// the object history on the UI thread or report `failure + message`.
    fn object_command(self: &Arc<Self>, target: Arc<AppReplica>, pending: Arc<PendingObjectHistory>, commands: Vec<Vec<String>>, input: Option<Value>, failure: &'static str) {
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = (|| -> AppResult<State> {
                for args in &commands {
                    target.command(crate::app_replica::CliArgs(&argv(args)), input.as_ref())?;
                }
                target.read()
            })();
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.finish_object_command(&pending, &snapshot)
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.remove_pending(&pending);
                    app.save_error.set(Some(error.to_string()));
                    app.update()?;
                    app.toast(&format!("{failure}{}", error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
    }
    /// `void fillEntity(String kind, String id, int fillColor)`.
    pub fn fill_entity(self: &Arc<Self>, kind: &ids::EntityKind, id: &ids::EntityId, fill_color: notemaker_core::style::ArgbColor) -> AppResult<()> {
        let (kind, id, fill_color): (&str, &str, i32) = (kind, id, fill_color.0);
        if kind != "object" && kind != "stroke" {
            return Ok(());
        }
        let target = self.replica();
        let pending = {
            let _sync = self.lock.enter();
            let pending = self.add_pending();
            self.saving_add(1);
            self.update()?;
            pending
        };
        let input = Value::Object(object! {kind => id, "color" => fill_color, "layer" => self.selected.get(), "page" => self.page.get()});
        self.object_command(target, pending, vec![owned(&["fill", "-"])], Some(input), "Could not fill area: ");
        Ok(())
    }
    /// `void renameLayer(String id)`.
    pub fn rename_layer(self: &Arc<Self>, id: &ids::LayerId) -> AppResult<()> {
        let id: &str = id;
        let Some(old) = self.layers.with(|l| l.get(id).cloned()) else { return Ok(()) };
        let name = self.edit_text()?;
        jcall!(name, "android/widget/TextView", "setSingleLine", "(Z)V", true)?;
        ui::set_text(&name, &ui::UiText::from(&old.opt_string("name")))?;
        let name_ref = WeakObj::new(&name);
        let id = id.to_string();
        let save = self.dialog_listener(move |app, _| {
            let value = jfmt::java_trim(&String::from(ui::text_of(&name_ref.get())?)).to_string();
            if value.is_empty() {
                return Ok(());
            }
            let _sync = app.lock.enter();
            app.enqueue(&Self::obj(&[("kind", JV::from("layer")), ("layer", JV::from(id.as_str())), ("name", JV::from(value)), ("force", JV::Bool(old.opt_boolean("force")))]))
        });
        let b = self.dialog_builder()?;
        jcall!(b, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", "Rename layer")?;
        jcall!(b, "android/app/AlertDialog$Builder", "setView", "(Landroid/view/View;)Landroid/app/AlertDialog$Builder;", &name)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Cancel", jx::NULL)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Save", &save)?;
        jcall!(b, "android/app/AlertDialog$Builder", "show", "()Landroid/app/AlertDialog;")?;
        Ok(())
    }
    /// `void layerCommand(String command, String id, String argument)`.
    pub fn layer_command(self: &Arc<Self>, command: LayerCommand, id: &ids::LayerId) -> AppResult<()> {
        let (command, argument) = match command {
            LayerCommand::Hide => ("hide", None),
            LayerCommand::Duplicate => ("duplicate", None),
            LayerCommand::Reorder(position) => ("reorder", Some(position.0.to_string())),
            LayerCommand::Merge(target) => ("merge", Some(String::from(target))),
        };
        let mut args = owned(&["layer", command, id]);
        if let Some(a) = argument {
            args.push(a);
        }
        let target = self.replica();
        let pending = {
            let _sync = self.lock.enter();
            let pending = self.add_pending();
            self.saving_add(1);
            self.update()?;
            pending
        };
        self.object_command(target, pending, vec![args], None, "Could not update layer: ");
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Selection and transforms (UI thread; commands on ReplicaService.writes)

    /// `ReplicaService.writes.execute(()->{try{target.command(["selection","deselect",acquired],null);}catch(...){Log.w(...)}})`.
    fn release_stale_selection(target: Arc<AppReplica>, acquired: String) {
        replica_service_writes().execute(move || {
            if let Err(error) = target.command(crate::app_replica::CliArgs(&["selection", "deselect", &acquired]), None) {
                jx::log::w_err(crate::jx::log::NOTEMAKER, "Could not release stale selection", &error);
            }
        });
    }
    /// `void startSelection(List<Map<String,Object>> entities)`.
    pub fn start_selection(self: &Arc<Self>, entities: Vec<Map>) -> AppResult<()> {
        if entities.is_empty() {
            return Ok(());
        }
        self.selection_error.set(None);
        self.selecting.set(true);
        let generation = self.selection_generation.get();
        let target = self.replica();
        let prior = self.selection_session.get();
        let request_sequence = if prior.is_none() {
            self.selection_request_sequence.with_mut(|s| {
                *s += 1;
                *s
            })
        } else {
            self.selection_request_sequence.get()
        };
        let mut input = object! {"layer" => self.selected.get(), "page" => self.page.get(), "entities" => Value::Array(entities.into_iter().map(Value::Object).collect())};
        if let Some(p) = &prior {
            input.insert("session".into(), Value::String(p.clone()));
        }
        let app = self.clone();
        replica_service_writes().execute(move || {
            let input = Value::Object(input);
            let result = target.command(crate::app_replica::CliArgs(&["selection", "select", "-"]), Some(&input)).and_then(into_map);
            match result {
                Ok(result) => app.run_on_ui_thread(move |app| {
                    let acquired = json::java_string(result.get("session"));
                    if !app.is_current(&target) || generation != app.selection_generation.get() || !app.selecting.get() || (prior.is_none() && request_sequence != app.selection_request_sequence.get()) {
                        Self::release_stale_selection(target, acquired);
                        return Ok(());
                    }
                    app.selection_session.set(Some(acquired));
                    for raw in list_or(&result, "selected")? {
                        let entity = json::map(&raw)?.clone();
                        let exists = app.selected_entities.with(|s| s.iter().any(|old| json::objects_equal(old.get("kind"), entity.get("kind")) && json::objects_equal(old.get("id"), entity.get("id"))));
                        if !exists {
                            app.selected_entities.with_mut(|s| s.push(entity));
                        }
                    }
                    let skipped = list_or(&result, "skipped")?;
                    app.selection_skipped.with_mut(|s| *s += skipped.len() as i32);
                    if !skipped.is_empty() {
                        app.toast(&format!("{} item(s) are locked by another editor", skipped.len()), TOAST_LENGTH_LONG)?;
                    }
                    app.schedule_heartbeat()?;
                    app.update()
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.selection_error.set(Some(error.to_string()));
                    app.toast(&format!("Could not select entities: {}", error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
        Ok(())
    }
    /// `LayerEditor layerEditor()`.
    pub fn layer_editor(&self) -> AppResult<LayerEditor> {
        Ok(LayerEditor::new(&self.model(), &ids::LayerId::from(self.selected.get()), ids::PageNumber(self.page.get()))?)
    }
    /// `void selectRegion(double[][] region, boolean topmost)` (`synchronized(lock)`; `null` region = all).
    fn select_region(self: &Arc<Self>, region: Option<&geometry::Polygon>, topmost: bool) -> AppResult<()> {
        let _sync = self.lock.enter();
        let selected = self.selected.get();
        match self.canvas.get() {
            Some(c) if c.visible(&ids::LayerId::from(&selected)) => {}
            _ => return Ok(()),
        }
        let hits = if topmost { notemaker_core::layer_editor::HitMode::Topmost } else { notemaker_core::layer_editor::HitMode::All };
        let hits = LayerEditor::select_in(&self.model(), &ids::LayerId::from(&selected), ids::PageNumber(self.page.get()), region, hits, &item_coverage::FACTORY)?;
        let mut refs = Vec::new();
        for hit in hits {
            refs.push(into_map(hit)?);
        }
        self.start_selection(refs)
    }
    /// `6/Math.max(.01,canvas.scale()*canvas.zoom)`.
    fn hit_radius(canvas: &CanvasView) -> f64 {
        6.0 / jmath::max(0.01, (canvas.scale().0 * canvas.zoom.get()) as f64)
    }
    /// `void selectAt(float x, float y)`.
    pub fn select_at(self: &Arc<Self>, at: DocPointF) -> AppResult<()> {
        let radius = match self.canvas.get() {
            None => 1.0,
            Some(c) => Self::hit_radius(&c),
        };
        let at = geometry::DocPoint::from(at);
        let region = geometry::Polygon::capsule(geometry::Segment { start: at, end: at }, geometry::Radius(radius));
        self.select_region(Some(&region), true)
    }
    /// `void selectAllVisible()`.
    pub fn select_all_visible(self: &Arc<Self>) -> AppResult<()> {
        self.select_region(None, false)
    }
    /// `void selectWithin(float x0, float y0, float x1, float y1)`.
    fn select_within(self: &Arc<Self>, x0: f32, y0: f32, x1: f32, y1: f32) -> AppResult<()> {
        let region = geometry::DocRect { x: fmin(x0, x1) as f64, y: fmin(y0, y1) as f64, width: (x1 - x0).abs() as f64, height: (y1 - y0).abs() as f64 }.polygon();
        self.select_region(Some(&region), false)
    }
    /// `private void addLassoPoint(float x, float y)`.
    fn add_lasso_point(&self, x: f32, y: f32) {
        let Some(canvas) = self.canvas.get() else { return };
        canvas.lasso_path.with_mut(|path| {
            let far = match path.last() {
                None => true,
                Some(last) => jmath::hypot((last[0] - x) as f64, (last[1] - y) as f64) >= 3.5f32 as f64,
            };
            if far {
                path.push([x, y]);
            }
        });
    }
    /// `void selectLasso(List<float[]> polygon)`.
    fn select_lasso(self: &Arc<Self>, polygon: &[[f32; 2]]) -> AppResult<()> {
        if polygon.len() < 3 {
            return Ok(());
        }
        let p: geometry::Polygon = polygon.iter().map(|q| geometry::DocPoint { x: q[0] as f64, y: q[1] as f64 }).collect();
        self.select_region(Some(&p), false)
    }
    /// `Map<String,Object> selectedEntityAt(float x, float y)` (`null` → `None`).
    pub fn selected_entity_at(&self, at: DocPointF) -> AppResult<Option<Map>> {
        let canvas = self.canvas.get().ok_or_else(npe)?;
        let radius = Self::hit_radius(&canvas);
        let at = geometry::DocPoint::from(at);
        let region = geometry::Polygon::capsule(geometry::Segment { start: at, end: at }, geometry::Radius(radius));
        let model = self.model();
        let selected = self.selected.get();
        let page = self.page.get();
        error::require(model.layers.contains_key(&selected) && !is_false_in(&model.layer_active, &selected), "Layer is not editable")?;
        error::require((0..=10000).contains(&page), "Invalid page")?;
        let refs = self.selected_entities.get();
        for r in refs {
            let kind = json::java_string(r.get("kind"));
            let id = json::java_string(r.get("id"));
            let source = match kind.as_str() {
                "stroke" => model.strokes.get(&id),
                "object" => model.objects.get(&id),
                _ => None,
            };
            let Some(source) = source else { continue };
            let table = if kind == "stroke" { &model.active } else { &model.object_active };
            if is_false_in(table, &id) || !same_str(source, "layer", &selected) || json::as_int(source.get("page"))? != page {
                continue;
            }
            let hit = DrawItem::new(&model, source.clone()).and_then(|item| item.intersects(&region, &item_coverage::FACTORY));
            match hit {
                Ok(true) => return Ok(Some(r)),
                Ok(false) => {}
                Err(stale) if AppError::from(stale.clone()).is("java.lang.IllegalArgumentException") => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(None)
    }
    /// `void selectLayer(String layer)`.
    pub fn select_layer(self: &Arc<Self>, layer: &ids::LayerId) -> AppResult<()> {
        let layer: &str = layer;
        if self.selected.get() != layer {
            let selection_tool = self.selecting.get();
            self.deselect()?;
            self.reset_drawing_gesture()?;
            self.selected.set(layer.to_string());
            self.selecting.set(selection_tool);
        }
        self.save();
        self.update()
    }
    /// `void deselectEntity(String kind, String id)`.
    pub fn deselect_entity(self: &Arc<Self>, kind: &ids::EntityKind, id: &ids::EntityId) -> AppResult<()> {
        let (kind, id): (&str, &str) = (kind, id);
        let Some(session) = self.selection_session.get() else { return Ok(()) };
        let remaining: Vec<Map> = self.selected_entities.with(|s| s.iter().filter(|e| !(same_str(e, "kind", kind) && same_str(e, "id", id))).cloned().collect());
        let target = self.replica();
        let generation = self.selection_generation.get();
        self.selection_session.set(None);
        self.selected_entities.with_mut(Vec::clear);
        self.pending_transform.set(None);
        self.selecting.set(true);
        self.remove_task(&self.heartbeat_task)?;
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = (|| -> AppResult<()> {
                target.command(crate::app_replica::CliArgs(&["selection", "deselect", &session]), None)?;
                if !remaining.is_empty() {
                    if generation != app.selection_generation.get() || !app.selecting.get() || !app.is_current(&target) {
                        return Ok(());
                    }
                    let request = Value::Object(object! {"layer" => app.selected.get(), "page" => app.page.get(), "entities" => Value::Array(remaining.into_iter().map(Value::Object).collect())});
                    let result = into_map(target.command(crate::app_replica::CliArgs(&["selection", "select", "-"]), Some(&request))?)?;
                    let acquired = json::java_string(result.get("session"));
                    let target = target.clone();
                    app.run_on_ui_thread(move |app| {
                        if !app.is_current(&target) || generation != app.selection_generation.get() || !app.selecting.get() {
                            Self::release_stale_selection(target, acquired);
                            return Ok(());
                        }
                        app.selection_session.set(Some(acquired));
                        for raw in list_or(&result, "selected")? {
                            let entity = json::map(&raw)?.clone();
                            app.selected_entities.with_mut(|s| s.push(entity));
                        }
                        let skipped = list_or(&result, "skipped")?.len() as i32;
                        app.selection_skipped.with_mut(|s| *s += skipped);
                        app.schedule_heartbeat()?;
                        app.update()
                    });
                } else {
                    app.run_on_ui_thread(|app| app.update());
                }
                Ok(())
            })();
            if let Err(error) = result {
                app.run_on_ui_thread(move |app| app.toast(&format!("Could not release item lock: {}", error.message_or_null()), TOAST_LENGTH_LONG));
            }
        });
        self.update()
    }
    /// `private void scheduleHeartbeat()`.
    pub fn schedule_heartbeat(self: &Arc<Self>) -> AppResult<()> {
        self.remove_task(&self.heartbeat_task)?;
        let Some(session) = self.selection_session.get() else { return Ok(()) };
        let target = self.replica();
        // Stored in a field and re-posted: capture the activity weakly.
        let weak = Arc::downgrade(self);
        let task = jx::try_runnable(move || {
            let Some(app) = weak.upgrade() else { return Ok(()) };
            if !app.is_current(&target) || app.selection_session.get().as_deref() != Some(session.as_str()) || !app.running.get() {
                return Ok(());
            }
            let (target, session) = (target.clone(), session.clone());
            replica_service_writes().execute(move || {
                if let Err(error) = target.command(crate::app_replica::CliArgs(&["selection", "heartbeat", &session]), None) {
                    jx::log::w_err(crate::jx::log::NOTEMAKER, "Selection heartbeat failed", &error);
                }
            });
            if let Some(task) = app.heartbeat_task.get() {
                ui::post_delayed(&app.selection_handler, &task, crate::jx::DelayMillis(5000))?;
            }
            Ok(())
        });
        self.heartbeat_task.set(Some(task.clone()));
        ui::post_delayed(&self.selection_handler, &task, crate::jx::DelayMillis(5000))?;
        Ok(())
    }
    /// `private void schedulePreviewExpiry()`.
    pub fn schedule_preview_expiry(self: &Arc<Self>) -> AppResult<()> {
        if let Some(task) = self.preview_expiry_task.get() {
            ui::remove_callbacks(&self.selection_handler, &task)?;
            self.preview_expiry_task.set(None);
        }
        if self.transform_previews.with(Map::is_empty) || self.canvas.with(Option::is_none) {
            return Ok(());
        }
        let weak = Arc::downgrade(self);
        let task = jx::try_runnable(move || {
            let Some(app) = weak.upgrade() else { return Ok(()) };
            app.preview_expiry_task.set(None);
            if let Some(c) = app.canvas.get() {
                c.invalidate()?;
            }
            let now = jx::current_time_millis().0;
            let previews: Vec<Value> = app.transform_previews.with(|p| p.values().cloned().collect());
            for raw in previews {
                if let Ok(e) = expires(&raw) {
                    if e > now {
                        app.schedule_preview_expiry()?;
                        break;
                    }
                }
            }
            Ok(())
        });
        self.preview_expiry_task.set(Some(task.clone()));
        ui::post_delayed(&self.selection_handler, &task, crate::jx::DelayMillis(500))?;
        Ok(())
    }
    /// `void deselect()`.
    pub fn deselect(self: &Arc<Self>) -> AppResult<()> {
        self.selection_generation.with_mut(|g| *g += 1);
        self.selection_request_sequence.with_mut(|s| *s += 1);
        let session = self.selection_session.get();
        let clear_preview = session.is_some() && (self.pending_transform.with(Option::is_some) || self.preview_in_flight.get());
        let target = self.replica.get();
        self.remove_task(&self.heartbeat_task)?;
        self.heartbeat_task.set(None);
        self.remove_task(&self.preview_dispatch_task)?;
        self.preview_dispatch_task.set(None);
        self.selection_session.set(None);
        self.selected_entities.with_mut(Vec::clear);
        self.selection_skipped.set(0);
        self.pending_transform.set(None);
        self.transform_commit_queued.set(false);
        self.selecting.set(false);
        if let Some(s) = &session {
            self.transform_previews.with_mut(|p| {
                p.shift_remove(s);
            });
        }
        if let Some(c) = self.canvas.get() {
            c.lassoing.set(false);
            c.lasso_path.with_mut(Vec::clear);
            c.transforming.set(false);
        }
        self.update()?;
        if let Some(session) = session {
            let target = target.ok_or_else(npe)?;
            replica_service_writes().execute(move || {
                let result = (|| -> AppResult<()> {
                    if clear_preview {
                        target.command(crate::app_replica::CliArgs(&["transform", "cancel", &session]), None)?;
                    }
                    target.command(crate::app_replica::CliArgs(&["selection", "deselect", &session]), None)?;
                    Ok(())
                })();
                if let Err(error) = result {
                    jx::log::w_err(crate::jx::log::NOTEMAKER, "Could not release selection", &error);
                }
            });
        }
        Ok(())
    }
    /// `void previewSelection(double scale, double dx, double dy)`.
    pub fn preview_selection(self: &Arc<Self>, transform: SelectionTransform) -> AppResult<()> {
        let SelectionTransform { scale, dx, dy } = transform;
        self.preview_transform(&object! {"scale" => json::double(scale), "dx" => json::double(dx), "dy" => json::double(dy)})
    }
    /// `void previewTransform(Map<String,Object> specification)`.
    pub fn preview_transform(self: &Arc<Self>, specification: &Map) -> AppResult<()> {
        let Some(session) = self.selection_session.get() else { return Ok(()) };
        if self.selected_entities.with(Vec::is_empty) || self.transform_commit_queued.get() {
            return Ok(());
        }
        let entities = self.selected_entities.with(|s| Value::Array(s.iter().cloned().map(Value::Object).collect()));
        self.pending_transform.set(Some(object! {"session" => session, "entities" => entities, "transform" => specification.clone()}));
        self.update()?;
        if !self.preview_in_flight.get() && self.preview_dispatch_task.with(Option::is_none) {
            let weak = Arc::downgrade(self);
            let task = jx::try_once_runnable(move || {
                let Some(app) = weak.upgrade() else { return Ok(()) };
                app.preview_dispatch_task.set(None);
                app.dispatch_latest_preview().map_err(JavaError::from)
            });
            self.preview_dispatch_task.set(Some(task.clone()));
            ui::post_delayed(&self.selection_handler, &task, crate::jx::DelayMillis(100))?;
        }
        Ok(())
    }
    /// `private void dispatchLatestPreview()`.
    pub fn dispatch_latest_preview(self: &Arc<Self>) -> AppResult<()> {
        if self.preview_in_flight.get() || self.transform_commit_queued.get() || self.selection_session.with(Option::is_none) {
            return Ok(());
        }
        let Some(sent) = self.pending_transform.get() else { return Ok(()) };
        let target = self.replica();
        let generation = self.selection_generation.get();
        self.preview_in_flight.set(true);
        let app = self.clone();
        replica_service_writes().execute(move || {
            let input = Value::Object(sent.clone());
            let result = target.command(crate::app_replica::CliArgs(&["transform", "preview", "-"]), Some(&input)).and_then(|_| target.read());
            let again = move |app: &Arc<MainActivity>, sent: &Map| -> AppResult<()> {
                if generation == app.selection_generation.get() && !app.transform_commit_queued.get() && app.pending_transform.with(|p| p.as_ref().is_some_and(|p| p != sent)) {
                    app.dispatch_latest_preview()?;
                }
                Ok(())
            };
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    app.preview_in_flight.set(false);
                    if app.is_current(&target) && generation == app.selection_generation.get() && json::objects_equal(app.selection_session.get().map(Value::String).as_ref(), sent.get("session")) {
                        let refreshed = app.refresh_shared(&snapshot).and_then(|_| app.update());
                        if let Err(error) = refreshed {
                            app.save_error.set(Some(error.to_string()));
                            app.update()?;
                        }
                    }
                    again(app, &sent)
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.preview_in_flight.set(false);
                    if generation == app.selection_generation.get() {
                        app.toast(&format!("Could not preview transform: {}", error.message_or_null()), TOAST_LENGTH_LONG)?;
                    }
                    again(app, &sent)
                }),
            }
        });
        Ok(())
    }
    /// `void transformSelection(double scale, double dx, double dy, boolean commit)`.
    fn transform_selection(self: &Arc<Self>, transform: SelectionTransform, commit: TransformCommit) -> AppResult<()> {
        if commit == TransformCommit::Preview {
            return self.preview_selection(transform);
        }
        self.preview_selection(transform)?;
        self.commit_transform_preview()
    }
    /// `void commitTransformPreview()`.
    pub fn commit_transform_preview(self: &Arc<Self>) -> AppResult<()> {
        let Some(transform) = self.pending_transform.get() else { return Ok(()) };
        if self.selection_session.with(Option::is_none) {
            return Ok(());
        }
        if let Some(task) = self.preview_dispatch_task.get() {
            ui::remove_callbacks(&self.selection_handler, &task)?;
            self.preview_dispatch_task.set(None);
        }
        self.transform_commit_queued.set(true);
        let target = self.replica();
        let pending = {
            let _sync = self.lock.enter();
            let pending = self.add_pending();
            self.saving_add(1);
            self.update()?;
            pending
        };
        let app = self.clone();
        replica_service_writes().execute(move || {
            let input = Value::Object(transform);
            let result = (|| -> AppResult<State> {
                target.command(crate::app_replica::CliArgs(&["transform", "preview", "-"]), Some(&input))?;
                target.command(crate::app_replica::CliArgs(&["transform", "commit", "-"]), Some(&input))?;
                target.read()
            })();
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.remove_pending(&pending);
                    let index = app.insert_object_action(&pending);
                    app.shift_pending(index);
                    app.pending_transform.set(None);
                    app.transform_commit_queued.set(false);
                    app.save_error.set(None);
                    app.refresh_or_record(&snapshot);
                    app.save();
                    app.update()
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.remove_pending(&pending);
                    app.transform_commit_queued.set(false);
                    app.save_error.set(Some(error.to_string()));
                    app.update()?;
                    app.toast(&format!("Could not transform selection: {}", error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
        Ok(())
    }
    /// `void cancelTransformPreview()`.
    pub fn cancel_transform_preview(self: &Arc<Self>) -> AppResult<()> {
        let Some(session) = self.selection_session.get() else { return Ok(()) };
        if self.pending_transform.with(Option::is_none) {
            return Ok(());
        }
        let target = self.replica();
        if let Some(task) = self.preview_dispatch_task.get() {
            ui::remove_callbacks(&self.selection_handler, &task)?;
            self.preview_dispatch_task.set(None);
        }
        self.pending_transform.set(None);
        self.transform_commit_queued.set(false);
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = target.command(crate::app_replica::CliArgs(&["transform", "cancel", &session]), None).and_then(|_| target.read());
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    if app.is_current(&target) {
                        if let Err(error) = app.refresh_shared(&snapshot).and_then(|_| app.update()) {
                            app.save_error.set(Some(error.to_string()));
                            app.update()?;
                        }
                    }
                    Ok(())
                }),
                Err(error) => app.run_on_ui_thread(move |app| app.toast(&format!("Could not cancel preview: {}", error.message_or_null()), TOAST_LENGTH_SHORT)),
            }
        });
        self.update()
    }
    /// `void resolveConflict(String id, String choice)`.
    pub fn resolve_conflict(self: &Arc<Self>, id: &ids::ConflictId, choice: ids::ConflictChoice) -> AppResult<()> {
        let id: &str = id;
        let choice = match choice {
            ids::ConflictChoice::Local => "local",
            ids::ConflictChoice::Remote => "remote",
            ids::ConflictChoice::Both => "both",
        };
        let target = self.replica();
        self.saving_add(1);
        self.update()?;
        let args = owned(&["conflict", "resolve", id, choice]);
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = target.command(crate::app_replica::CliArgs(&argv(&args)), None).and_then(|_| target.read());
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.save_error.set(None);
                    app.refresh_or_record(&snapshot);
                    app.update()
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.save_error.set(Some(error.to_string()));
                    app.update()?;
                    app.toast(&format!("Could not resolve conflict: {}", error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
        Ok(())
    }
    /// `void duplicateSelection()`.
    pub fn duplicate_selection(self: &Arc<Self>) -> AppResult<()> {
        if self.selection_session.with(Option::is_some) {
            self.selection_command("duplicate")?;
        }
        Ok(())
    }
    /// `void deleteSelection()`.
    pub fn delete_selection(self: &Arc<Self>) -> AppResult<()> {
        if self.selection_session.with(Option::is_some) {
            self.selection_command("delete")?;
        }
        Ok(())
    }
    /// `void copySelection()`.
    pub fn copy_selection(self: &Arc<Self>) -> AppResult<()> {
        self.selection_clipboard_command("copy")
    }
    /// `void cutSelection()`.
    pub fn cut_selection(self: &Arc<Self>) -> AppResult<()> {
        self.selection_clipboard_command("cut")
    }
    /// `void pasteSelection()`.
    pub fn paste_selection(self: &Arc<Self>) -> AppResult<()> {
        let entities = list_or(&self.selection_clipboard.get(), "entities")?;
        if entities.is_empty() {
            return self.toast("Clipboard is empty", TOAST_LENGTH_SHORT);
        }
        let input = Value::Object(object! {"entities" => entities, "layer" => self.selected.get(), "page" => self.page.get(), "dx" => 24, "dy" => 24});
        let target = self.replica();
        self.saving_add(1);
        self.update()?;
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = target.command(crate::app_replica::CliArgs(&["selection", "paste", "-"]), Some(&input)).and_then(|_| target.read());
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.undo.with_mut(|u| u.push("object-action".into()));
                    app.redo.with_mut(Vec::clear);
                    app.save_error.set(None);
                    app.refresh_or_record(&snapshot);
                    app.save();
                    app.update()
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.save_error.set(Some(error.to_string()));
                    app.update()?;
                    app.toast(&format!("Could not paste selection: {}", error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
        Ok(())
    }
    /// `private void selectionClipboardCommand(String command)`.
    fn selection_clipboard_command(self: &Arc<Self>, command: &str) -> AppResult<()> {
        let Some(session) = self.selection_session.get() else { return Ok(()) };
        if self.selected_entities.with(Vec::is_empty) {
            return Ok(());
        }
        let target = self.replica();
        self.saving_add(1);
        self.update()?;
        let command = command.to_string();
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = (|| -> AppResult<(Map, State)> {
                let result = into_map(target.command(crate::app_replica::CliArgs(&["selection", &command, &session]), None)?)?;
                let clipboard = object! {"entities" => list_or(&result, "entities")?, "copiedAt" => jx::current_time_millis().0};
                let snapshot = target.read()?;
                Ok((clipboard, snapshot))
            })();
            match result {
                Ok((clipboard, snapshot)) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.selection_clipboard.set(clipboard.clone());
                    app.put_pref("selectionClipboard", &json::write_map(&clipboard))?;
                    app.save_error.set(None);
                    if command == "cut" {
                        app.refresh_or_record(&snapshot);
                        app.undo.with_mut(|u| u.push("object-action".into()));
                        app.redo.with_mut(Vec::clear);
                        app.save();
                        app.deselect()?;
                    } else {
                        app.save();
                    }
                    app.update()
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.save_error.set(Some(error.to_string()));
                    app.update()?;
                    app.toast(&format!("Could not {} selection: {}", command, error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
        Ok(())
    }
    /// `private void selectionCommand(String command)`.
    fn selection_command(self: &Arc<Self>, command: &str) -> AppResult<()> {
        let Some(session) = self.selection_session.get() else { return Ok(()) };
        let target = self.replica();
        let pending = {
            let _sync = self.lock.enter();
            let pending = self.add_pending();
            self.saving_add(1);
            self.update()?;
            pending
        };
        let command = command.to_string();
        let app = self.clone();
        replica_service_writes().execute(move || {
            let result = target.command(crate::app_replica::CliArgs(&["selection", &command, &session]), None).and_then(|_| target.read());
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.remove_pending(&pending);
                    let index = app.insert_object_action(&pending);
                    app.shift_pending(index);
                    app.save_error.set(None);
                    app.refresh_or_record(&snapshot);
                    app.save();
                    if command == "delete" {
                        app.deselect()?;
                    }
                    app.update()
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.remove_pending(&pending);
                    app.save_error.set(Some(error.to_string()));
                    app.update()?;
                    app.toast(&format!("Could not update selection: {}", error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Drawing actions

    /// `void applyErase(String mode, String layer, int erasePage, JSONArray path, float radius)`.
    pub fn apply_erase(self: &Arc<Self>, mode: &EraserMode, layer: &ids::LayerId, erase_page: ids::PageNumber, path: Option<&JSONArray>, radius: geometry::Radius) -> AppResult<()> {
        let (mode, layer, erase_page): (&str, &str, i32) = (mode, layer, erase_page.0);
        // Callers pass the editor's `float` eraser radius, so this is exact.
        let radius = radius.0 as f32;
        let trace = self.erase_timings.get();
        trace_put(&trace, "entered");
        let target = self.replica();
        let preview_id = String::from(Self::id());
        let points = match path {
            None => Value::Array(Vec::new()),
            Some(p) => json::parse(&p.to_string())?,
        };
        let input = object! {"mode" => mode, "layer" => layer, "page" => erase_page, "points" => points, "radius" => json::from_float(radius)};
        // Do not predict an edit when coordination state advertises a foreign lock.
        let foreign = {
            let leases: Vec<Value> = self.leases.with(|l| l.values().cloned().collect());
            let mut foreign = false;
            for raw in leases {
                if !same_str(json::map(&raw)?, "actor", "human") {
                    foreign = true;
                    break;
                }
            }
            foreign
        };
        if !foreign {
            match ErasePreview::new(&self.model(), layer, erase_page, &input) {
                Ok(preview) => {
                    let canvas = self.canvas.get().ok_or_else(npe)?;
                    canvas.erase_previews.with_mut(|p| {
                        p.insert(preview_id.clone(), preview);
                    });
                }
                Err(e) if e.is("java.lang.IllegalArgumentException") => {}
                Err(e) => return Err(e),
            }
        }
        let pending = {
            let _sync = self.lock.enter();
            let pending = self.add_pending();
            self.saving_add(1);
            self.update()?;
            pending
        };
        trace_put(&trace, "queued");
        let app = self.clone();
        replica_service_writes().execute(move || {
            trace_put(&trace, "worker_start");
            let result = (|| -> AppResult<State> {
                if let Some(t) = &trace {
                    let t = t.clone();
                    timing::set_observer(Some(Box::new(move |phase: &str| {
                        let now = jx::nano_time().0;
                        t.with_mut(|m| {
                            m.insert(phase.to_string(), now);
                        });
                    })));
                }
                let input = Value::Object(input);
                let erased = target.command(crate::app_replica::CliArgs(&["erase", "-"]), Some(&input));
                timing::set_observer(None);
                erased?;
                trace_put(&trace, "command_done");
                let snapshot = target.read()?;
                trace_put(&trace, "snapshot_done");
                Ok(snapshot)
            })();
            match result {
                Ok(snapshot) => app.run_on_ui_thread(move |app| {
                    trace_put(&trace, "ui_start");
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.canvas().erase_previews.with_mut(|p| {
                        p.shift_remove(&preview_id);
                    });
                    app.remove_pending(&pending);
                    let index = app.insert_object_action(&pending);
                    app.redo.with_mut(Vec::clear);
                    app.shift_pending(index);
                    app.save_error.set(None);
                    app.refresh_or_record(&snapshot);
                    trace_put(&trace, "refresh_done");
                    app.save();
                    app.update()?;
                    trace_put(&trace, "ui_done");
                    Ok(())
                }),
                Err(error) => app.run_on_ui_thread(move |app| {
                    app.saving_add(-1);
                    if !app.is_current(&target) {
                        return Ok(());
                    }
                    app.canvas().erase_previews.with_mut(|p| {
                        p.shift_remove(&preview_id);
                    });
                    app.remove_pending(&pending);
                    if error.message().as_deref() == Some("No matching editable content") {
                        return app.update();
                    }
                    app.save_error.set(Some(error.to_string()));
                    app.update()?;
                    app.toast(&format!("Could not erase: {}", error.message_or_null()), TOAST_LENGTH_LONG)
                }),
            }
        });
        Ok(())
    }
    /// `void fillAt(float x, float y)`.
    pub fn fill_at(self: &Arc<Self>, at: DocPointF) -> AppResult<()> {
        let DocPointF { x, y } = at;
        let target = self.replica();
        let pending = {
            let _sync = self.lock.enter();
            let pending = self.add_pending();
            self.saving_add(1);
            pending
        };
        let (app, check_target, check_pending) = (self.clone(), target.clone(), pending.clone());
        let chrome = jx::try_once_runnable(move || {
            if app.is_current(&check_target) && app.pending_object_history.with(|l| l.iter().any(|p| Arc::ptr_eq(p, &check_pending))) {
                app.update_chrome()?;
            }
            Ok(())
        });
        ui::post_delayed(&self.selection_handler, &chrome, crate::jx::DelayMillis(150))?;
        let fill = self.object_fill_color.get();
        let input = Value::Object(object! {"x" => json::from_float(x), "y" => json::from_float(y), "page" => self.page.get(), "layer" => self.selected.get(), "color" => if fill == COLOR_TRANSPARENT { self.color.get() } else { fill }});
        self.object_command(target, pending, vec![owned(&["fill", "-"])], Some(input), "Could not fill area: ");
        Ok(())
    }
    /// `private void insertSvg(String source, double intrinsicWidth, double intrinsicHeight)`.
    fn insert_svg(self: &Arc<Self>, source: &str, intrinsic_width: f64, intrinsic_height: f64) -> AppResult<()> {
        self.insert_svg_template(source, intrinsic_width, intrinsic_height, false)
    }
    /// The shared `(1000-w)/2`, `(1400-h)/2` placement of the import helpers.
    fn placed_object(&self, shape: &str, data: Option<(&str, &str)>, w: f32, h: f32, trailing: Option<(&str, &str)>) -> JSONObject {
        let mut args: Vec<(&str, JV)> = vec![("kind", JV::from("object")), ("object", JV::from(String::from(Self::id()))), ("shape", JV::from(shape))];
        if let Some((k, v)) = data {
            args.push((k, JV::from(v)));
        }
        args.extend([
            ("layer", JV::from(self.selected.get())),
            ("page", JV::Int(self.page.get())),
            ("x", JV::Float((1000.0 - w) / 2.0)),
            ("y", JV::Float((1400.0 - h) / 2.0)),
            ("w", JV::Float(w)),
            ("h", JV::Float(h)),
            ("color", JV::Int(self.color.get())),
            ("width", JV::Float(self.width.get())),
            ("fontSize", JV::Int(24)),
            ("text", JV::from("")),
        ]);
        if let Some((k, v)) = trailing {
            args.push((k, JV::from(v)));
        }
        Self::obj(&args)
    }
    /// `private void insertSvg(String source, double intrinsicWidth, double intrinsicHeight, boolean template)`.
    fn insert_svg_template(self: &Arc<Self>, source: &str, intrinsic_width: f64, intrinsic_height: f64, template: bool) -> AppResult<()> {
        let scale = if template { 1.0 } else { jmath::min(800.0 / intrinsic_width, 1000.0 / intrinsic_height) };
        let (w, h) = ((intrinsic_width * scale) as f32, (intrinsic_height * scale) as f32);
        let object = self.placed_object("svg", None, w, h, Some(("svg", source)));
        self.persist_object(&object)
    }
    /// `private void insertImage(String data, double intrinsicWidth, double intrinsicHeight)`.
    fn insert_image(self: &Arc<Self>, data: &str, intrinsic_width: f64, intrinsic_height: f64) -> AppResult<()> {
        let scale = jmath::min(800.0 / intrinsic_width, 1000.0 / intrinsic_height);
        let (w, h) = ((intrinsic_width * scale) as f32, (intrinsic_height * scale) as f32);
        let object = self.placed_object("image", Some(("image", data)), w, h, None);
        self.persist_object(&object)
    }
    /// `private void insertGif(String data, double intrinsicWidth, double intrinsicHeight)`.
    fn insert_gif(self: &Arc<Self>, data: &str, intrinsic_width: f64, intrinsic_height: f64) -> AppResult<()> {
        let scale = jmath::min(800.0 / intrinsic_width, 1000.0 / intrinsic_height);
        let (w, h) = ((intrinsic_width * scale) as f32, (intrinsic_height * scale) as f32);
        let object = self.placed_object("gif", Some(("gif", data)), w, h, None);
        self.persist_object(&object)
    }
    /// `void textPlacementDialog()` (the two anonymous `TextView` subclasses
    /// `MainActivity$1/$2` only call `setText` in an initializer).
    pub fn text_placement_dialog(self: &Arc<Self>) -> AppResult<()> {
        let content = self.edit_text()?;
        jcall!(content, "android/widget/TextView", "setHint", "(Ljava/lang/CharSequence;)V", "Text to place")?;
        jcall!(content, "android/widget/TextView", "setMinLines", "(I)V", 2)?;
        jcall!(content, "android/widget/TextView", "setGravity", "(I)V", GRAVITY_TOP)?;
        let size = self.edit_text()?;
        jcall!(size, "android/widget/TextView", "setSingleLine", "(Z)V", true)?;
        jcall!(size, "android/widget/TextView", "setInputType", "(I)V", 8194 | 4096)?;
        ui::set_text(&size, &ui::UiText::from("24"))?;
        jcall!(size, "android/widget/TextView", "setHint", "(Ljava/lang/CharSequence;)V", "Font size")?;
        let bold = self.check_box("Bold", self.pending_text_bold.get())?;
        let italic = self.check_box("Italic", self.pending_text_italic.get())?;
        let underline = self.check_box("Underline", self.pending_text_underline.get())?;
        let family = self.spinner(&["sans-serif", "serif", "monospace"])?;
        let f = self.pending_text_family.get();
        jcall!(family, "android/widget/Spinner", "setSelection", "(I)V", if f == "serif" { 1 } else if f == "monospace" { 2 } else { 0 })?;
        let alignment = self.spinner(&["left", "center", "right"])?;
        let a = self.pending_text_alignment.get();
        jcall!(alignment, "android/widget/Spinner", "setSelection", "(I)V", if a == "center" { 1 } else if a == "right" { 2 } else { 0 })?;
        let box_ = ui::linear_layout(&self.this, crate::android_consts::Orientation::Vertical)?;
        ui::set_padding(&box_, ui::PxInsets { left: 24, top: 8, right: 24, bottom: 8 })?;
        ui::add_view(&box_, &content)?;
        ui::add_view(&box_, &size)?;
        ui::add_view(&box_, &bold)?;
        ui::add_view(&box_, &italic)?;
        ui::add_view(&box_, &underline)?;
        let font_label = ui::text_view(&self.this)?;
        ui::set_text(&font_label, &ui::UiText::from("Font"))?;
        ui::add_view(&box_, &font_label)?;
        ui::add_view(&box_, &family)?;
        let alignment_label = ui::text_view(&self.this)?;
        ui::set_text(&alignment_label, &ui::UiText::from("Alignment"))?;
        ui::add_view(&box_, &alignment_label)?;
        ui::add_view(&box_, &alignment)?;
        let views: Vec<WeakObj> = [&content, &size, &bold, &italic, &underline, &family, &alignment].iter().map(|v| WeakObj::new(v)).collect();
        let place = self.dialog_listener(move |app, _| {
            let result = (|| -> AppResult<()> {
                let v: Vec<Obj> = views.iter().map(WeakObj::get).collect();
                let value = String::from(ui::text_of(&v[0])?);
                let font = jfmt::parse_double(&String::from(ui::text_of(&v[1])?))? as f32;
                if jfmt::java_trim(&value).is_empty() || !font.is_finite() || font < 1.0 || font > 500.0 {
                    return Err(AppError::illegal_argument("Enter text and a font size from 1 to 500"));
                }
                app.pending_text.set(Some(value));
                app.pending_text_size.set(font);
                app.pending_text_bold.set(Self::is_checked(&v[2])?);
                app.pending_text_italic.set(Self::is_checked(&v[3])?);
                app.pending_text_underline.set(Self::is_checked(&v[4])?);
                app.pending_text_family.set(Self::selected_item(&v[5])?);
                app.pending_text_alignment.set(Self::selected_item(&v[6])?);
                app.set_drawing_tool(Some(&ids::ShapeId::from("text")))
            })();
            if let Err(e) = result {
                app.toast(&e.message().unwrap_or_default(), TOAST_LENGTH_LONG)?;
            }
            Ok(())
        });
        let b = self.dialog_builder()?;
        jcall!(b, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", "Place text")?;
        jcall!(b, "android/app/AlertDialog$Builder", "setMessage", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", "Enter text, then tap the canvas to position it.")?;
        jcall!(b, "android/app/AlertDialog$Builder", "setView", "(Landroid/view/View;)Landroid/app/AlertDialog$Builder;", &box_)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Cancel", jx::NULL)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Continue", &place)?;
        jcall!(b, "android/app/AlertDialog$Builder", "show", "()Landroid/app/AlertDialog;")?;
        Ok(())
    }
    /// `void resetDrawingGesture()`.
    pub fn reset_drawing_gesture(&self) -> AppResult<()> {
        self.drawing_shape.set(None);
        self.pending_text.set(None);
        if let Some(canvas) = self.canvas.get() {
            canvas.shape_dragging.set(false);
            canvas.set_points(None);
            canvas.current.set(None);
            canvas.eraser_points.set(None);
            canvas.invalidate()?;
        }
        Ok(())
    }
    /// `void editObject(String id)` (`synchronized(lock)`).
    pub fn edit_object(self: &Arc<Self>, id: &ids::EntityId) -> AppResult<()> {
        let id: &str = id;
        let _sync = self.lock.enter();
        let model = self.model();
        let Some(original) = model.objects.get(id).cloned() else { return Ok(()) };
        let edit_layer = match original.get("layer") {
            None => "base".to_string(),
            v => json::java_string(v),
        };
        let edit_page = int_or(&original, "page", 0)?;
        let form = ui::linear_layout(&self.this, crate::android_consts::Orientation::Vertical)?;
        ui::set_padding(&form, ui::PxInsets { left: 24, top: 12, right: 24, bottom: 12 })?;
        let mut fields: Vec<(String, WeakObj)> = Vec::new();
        let svg = same_str(&original, "shape", "svg");
        let keys: &[&str] = if svg { &["x", "y", "w", "h"] } else { &["text", "x", "y", "w", "h", "fontSize"] };
        let labels: &[&str] = if svg { &["X position", "Y position", "Width", "Height"] } else { &["Label", "X position", "Y position", "Width", "Height", "Font size"] };
        for (index, key) in keys.iter().enumerate() {
            let label = ui::text_view(&self.this)?;
            ui::set_text(&label, &ui::UiText::from(labels[index]))?;
            ui::add_view(&form, &label)?;
            let field = self.edit_text()?;
            let value = match original.get(*key) {
                Some(v) => json::java_string(Some(v)),
                None if *key == "text" => String::new(),
                None => "24".to_string(),
            };
            ui::set_text(&field, &ui::UiText::from(&value))?;
            if *key != "text" {
                jcall!(field, "android/widget/TextView", "setInputType", "(I)V", 8194 | 4096)?;
            }
            ui::add_view(&form, &field)?;
            fields.push((key.to_string(), WeakObj::new(&field)));
        }
        let mut text_controls: Option<[WeakObj; 5]> = None;
        if same_str(&original, "shape", "text") {
            let bold = self.check_box("Bold", json::is_true(original.get("bold")))?;
            ui::add_view(&form, &bold)?;
            let italic = self.check_box("Italic", json::is_true(original.get("italic")))?;
            ui::add_view(&form, &italic)?;
            let underline = self.check_box("Underline", json::is_true(original.get("underline")))?;
            ui::add_view(&form, &underline)?;
            let family = self.spinner(&["sans-serif", "serif", "monospace"])?;
            let ff = match original.get("fontFamily") {
                None => "sans-serif".to_string(),
                v => json::java_string(v),
            };
            jcall!(family, "android/widget/Spinner", "setSelection", "(I)V", if ff == "serif" { 1 } else if ff == "monospace" { 2 } else { 0 })?;
            ui::add_view(&form, &family)?;
            let alignment = self.spinner(&["left", "center", "right"])?;
            let al = match original.get("alignment") {
                None => "left".to_string(),
                v => json::java_string(v),
            };
            jcall!(alignment, "android/widget/Spinner", "setSelection", "(I)V", if al == "center" { 1 } else if al == "right" { 2 } else { 0 })?;
            ui::add_view(&form, &alignment)?;
            text_controls = Some([WeakObj::new(&bold), WeakObj::new(&italic), WeakObj::new(&underline), WeakObj::new(&family), WeakObj::new(&alignment)]);
        }
        let id_save = id.to_string();
        let (layer_save, page_save) = (edit_layer.clone(), edit_page);
        let save = self.dialog_listener(move |app, _| {
            let result = (|| -> AppResult<()> {
                let _sync = app.lock.enter();
                let mut patch = Map::new();
                for (key, field) in &fields {
                    let text = String::from(ui::text_of(&field.get())?);
                    let value = if key == "text" { Value::String(text) } else { json::double(jfmt::parse_double(&text)?) };
                    patch.insert(key.clone(), value);
                }
                if let Some(controls) = &text_controls {
                    patch.insert("bold".into(), Value::Bool(Self::is_checked(&controls[0].get())?));
                    patch.insert("italic".into(), Value::Bool(Self::is_checked(&controls[1].get())?));
                    patch.insert("underline".into(), Value::Bool(Self::is_checked(&controls[2].get())?));
                    patch.insert("fontFamily".into(), Value::String(Self::selected_item(&controls[3].get())?));
                    patch.insert("alignment".into(), Value::String(Self::selected_item(&controls[4].get())?));
                }
                patch.insert("scope".into(), Value::Object(object! {"layer" => layer_save.clone(), "page" => page_save}));
                app.persist(&["object", "update", &id_save, "-"], Some(Value::Object(patch)), None);
                app.undo.with_mut(|u| u.push("object-action".into()));
                app.redo.with_mut(Vec::clear);
                app.save();
                app.update()
            })();
            if let Err(e) = result {
                app.toast(&e.message().unwrap_or_default(), TOAST_LENGTH_LONG)?;
            }
            Ok(())
        });
        let hidden = is_false_in(&model.object_active, id);
        let id_toggle = id.to_string();
        let toggle = self.dialog_listener(move |app, _| {
            let result = (|| -> AppResult<()> {
                let _sync = app.lock.enter();
                let command = if app.model.with(|m| is_false_in(&m.object_active, &id_toggle)) { "show" } else { "hide" };
                app.persist(&["object", command, &id_toggle, &edit_layer, &edit_page.to_string()], None, None);
                app.undo.with_mut(|u| u.push("object-action".into()));
                app.redo.with_mut(Vec::clear);
                app.save();
                app.update()
            })();
            if let Err(e) = result {
                app.toast(&e.message().unwrap_or_default(), TOAST_LENGTH_LONG)?;
            }
            Ok(())
        });
        let b = self.dialog_builder()?;
        jcall!(b, "android/app/AlertDialog$Builder", "setTitle", "(Ljava/lang/CharSequence;)Landroid/app/AlertDialog$Builder;", "Edit object")?;
        jcall!(b, "android/app/AlertDialog$Builder", "setView", "(Landroid/view/View;)Landroid/app/AlertDialog$Builder;", &form)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setPositiveButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Save", &save)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setNeutralButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", if hidden { "Restore" } else { "Erase" }, &toggle)?;
        jcall!(b, "android/app/AlertDialog$Builder", "setNegativeButton", "(Ljava/lang/CharSequence;Landroid/content/DialogInterface$OnClickListener;)Landroid/app/AlertDialog$Builder;", "Cancel", jx::NULL)?;
        jcall!(b, "android/app/AlertDialog$Builder", "show", "()Landroid/app/AlertDialog;")?;
        Ok(())
    }
}

/// `class CanvasView extends View` (inner class of `MainActivity`).
///
/// The Java view is a `NativeView` (mask DRAW | TOUCH | DETACHED | SIZE) whose
/// handle closure owns an `Arc<CanvasView>`; this struct names its own view
/// weakly ([`CanvasView::view`]). `MainActivity.canvas` holds another `Arc`.
/// All callbacks run on the UI thread; `onDraw`/`onTouchEvent` take
/// `synchronized(lock)` of the activity.
pub struct CanvasView {
    pub app: Weak<MainActivity>,
    /// The Java `NativeView` (`this`), set once by [`CanvasView::new`].
    pub view: OnceLock<WeakObj>,
    pub erase_previews: Field<IndexMap<String, ErasePreview>>,
    /// `Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG)`.
    pub paint: Obj,
    pub zoom: Field<f32>,
    pub offset_x: Field<f32>,
    pub offset_y: Field<f32>,
    pub last_x: Field<f32>,
    pub last_y: Field<f32>,
    pub last_gesture_x: Field<f32>,
    pub last_gesture_y: Field<f32>,
    /// `JSONArray points`. Assign through [`CanvasView::set_points`], which
    /// keeps `point_values` in step.
    pub points: Field<Option<JSONArray>>,
    pub eraser_points: Field<Option<JSONArray>>,
    pub current: Field<Option<String>>,
    pub last_sent: Field<i64>,
    /// `ScaleGestureDetector scaling` (listener: `CanvasView$1.onScale`).
    pub scaling: Field<Option<Obj>>,
    pub tile_ink: Field<TileInkCache>,
    pub stroke_tile_items: Field<HashMap<String, Arc<tile_ink_cache::Item>>>,
    pub object_tile_sources: Field<HashMap<String, Entity>>,
    pub object_tile_items: Field<HashMap<String, Arc<tile_ink_cache::Item>>>,
    pub pruned_generation: Field<i64>,
    pub ink_document: Field<String>,
    pub ink_type: Field<String>,
    pub ink_page: Field<i32>,
    pub raster_tier: Field<tile_ink_cache::DensityTier>,
    pub input_started: Field<i64>,
    pub draw_latency: Field<Option<LongConsumer>>,
    pub after_draw: Field<Option<Callback>>,
    pub use_ink_cache: Field<bool>,
    pub cull_tile_items: Field<bool>,
    pub monolithic_tile_probe: Field<Option<RectF>>,
    pub shape_start_x: Field<f32>,
    pub shape_start_y: Field<f32>,
    pub shape_end_x: Field<f32>,
    pub shape_end_y: Field<f32>,
    pub shape_dragging: Field<bool>,
    pub transforming: Field<bool>,
    pub transform_start_x: Field<f32>,
    pub transform_start_y: Field<f32>,
    pub transform_delta_x: Field<f32>,
    pub transform_delta_y: Field<f32>,
    pub last_preview_at: Field<i64>,
    pub lassoing: Field<bool>,
    pub lasso_start_x: Field<f32>,
    pub lasso_start_y: Field<f32>,
    pub lasso_end_x: Field<f32>,
    pub lasso_end_y: Field<f32>,
    pub lasso_path: Field<Vec<[f32; 2]>>,
    /// Core JSON form (`[x, y, pressure]` of `Float`s) of every entry of
    /// `points`, appended alongside it (port-only cache for `strokeValue`).
    pub point_values: Field<Vec<Value>>,
    /// `getWidth()`/`getHeight()`, updated by `onSizeChanged`.
    pub view_width: AtomicI32,
    pub view_height: AtomicI32,
}

impl CanvasView {
    /// `CanvasView(Context context)`: creates the `NativeView`, background,
    /// and the `ScaleGestureDetector` with the `CanvasView$1` listener.
    pub fn new(app: &Arc<MainActivity>, context: &Obj) -> AppResult<Arc<CanvasView>> {
        let paint = jnew!("android/graphics/Paint", "(I)V", PAINT_ANTI_ALIAS_FLAG)?;
        let canvas = Arc::new(CanvasView {
            app: Arc::downgrade(app),
            view: OnceLock::new(),
            erase_previews: Field::new(IndexMap::new()),
            paint,
            zoom: Field::new(1.0),
            offset_x: Field::new(0.0),
            offset_y: Field::new(0.0),
            last_x: Field::new(0.0),
            last_y: Field::new(0.0),
            last_gesture_x: Field::new(0.0),
            last_gesture_y: Field::new(0.0),
            points: Field::new(None),
            eraser_points: Field::new(None),
            current: Field::new(None),
            last_sent: Field::new(0),
            scaling: Field::new(None),
            tile_ink: Field::new(TileInkCache::new()?),
            stroke_tile_items: Field::new(HashMap::new()),
            object_tile_sources: Field::new(HashMap::new()),
            object_tile_items: Field::new(HashMap::new()),
            pruned_generation: Field::new(-1),
            ink_document: Field::new(String::new()),
            ink_type: Field::new(String::new()),
            ink_page: Field::new(-1),
            raster_tier: Field::new(tile_ink_cache::DensityTier(1)),
            input_started: Field::new(0),
            draw_latency: Field::new(None),
            after_draw: Field::new(None),
            use_ink_cache: Field::new(true),
            cull_tile_items: Field::new(true),
            monolithic_tile_probe: Field::new(None),
            shape_start_x: Field::new(0.0),
            shape_start_y: Field::new(0.0),
            shape_end_x: Field::new(0.0),
            shape_end_y: Field::new(0.0),
            shape_dragging: Field::new(false),
            transforming: Field::new(false),
            transform_start_x: Field::new(0.0),
            transform_start_y: Field::new(0.0),
            transform_delta_x: Field::new(0.0),
            transform_delta_y: Field::new(0.0),
            last_preview_at: Field::new(0),
            lassoing: Field::new(false),
            lasso_start_x: Field::new(0.0),
            lasso_start_y: Field::new(0.0),
            lasso_end_x: Field::new(0.0),
            lasso_end_y: Field::new(0.0),
            lasso_path: Field::new(Vec::new()),
            point_values: Field::new(Vec::new()),
            view_width: AtomicI32::new(0),
            view_height: AtomicI32::new(0),
        });
        let owner = canvas.clone();
        let mask = jx::mask::DRAW | jx::mask::TOUCH | jx::mask::DETACHED | jx::mask::SIZE;
        let view = jx::native_view(context, mask, move |method, args| owner.dispatch(method, args))?;
        let _ = canvas.view.set(WeakObj::new(&view));
        ui::set_background_color(&view, notemaker_core::style::ArgbColor(COLOR_WHITE))?;
        let weak = Arc::downgrade(&canvas);
        let listener = jx::scale_listener(move |method, args| {
            if method == "onScale(ScaleGestureDetector)" {
                if let (Some(canvas), Some(detector)) = (weak.upgrade(), args.first().and_then(jx::global)) {
                    return Ok(Reply::Bool(canvas.on_scale(&detector).map_err(JavaError::from)?));
                }
            }
            Ok(Reply::Void)
        })?;
        let detector = jnew!("android/view/ScaleGestureDetector", "(Landroid/content/Context;Landroid/view/ScaleGestureDetector$OnScaleGestureListener;)V", context, &listener)?;
        canvas.scaling.set(Some(detector));
        Ok(canvas)
    }
    /// The Java view (`this`).
    pub fn view(&self) -> Obj {
        self.view.get().expect("CanvasView not constructed").get()
    }
    pub fn app(&self) -> Arc<MainActivity> {
        self.app.upgrade().expect("MainActivity dropped")
    }
    /// `invalidate()`.
    pub fn invalidate(&self) -> AppResult<()> {
        Ok(crate::ui::invalidate(&self.view())?)
    }
    /// `invalidate(l, t, r, b)`.
    pub fn invalidate_rect(&self, dirty: ScreenRect) -> AppResult<()> {
        let ScreenRect { left: l, top: t, right: r, bottom: b } = dirty;
        jcall!(self.view(), "android/view/View", "invalidate", "(IIII)V", l, t, r, b)?;
        Ok(())
    }
    /// `getWidth()` (tracked through `onSizeChanged`).
    pub fn width(&self) -> ui::Px {
        ui::Px(self.view_width.load(Ordering::SeqCst))
    }
    /// `getHeight()` (tracked through `onSizeChanged`).
    pub fn height(&self) -> ui::Px {
        ui::Px(self.view_height.load(Ordering::SeqCst))
    }
    /// `points = value` (also resets the parallel `point_values` cache).
    pub fn set_points(&self, value: Option<JSONArray>) {
        let values = match &value {
            None => Vec::new(),
            Some(points) => points.values().iter().map(Self::point_value).collect(),
        };
        self.point_values.set(values);
        self.points.set(value);
    }
    /// Core JSON value of one `points` entry (`Arrays.asList(point.opt(0), point.opt(1), point.opt(2))`).
    fn point_value(point: &JV) -> Value {
        let JV::Arr(point) = point else { return Value::Null };
        let v = |i: usize| point.opt(i).map(|v| org_json::unwrap(&v)).unwrap_or(Value::Null);
        Value::Array(vec![v(0), v(1), v(2)])
    }

    /// Native view callback dispatch (`onDraw(Canvas)`, `onTouchEvent(MotionEvent)`,
    /// `onDetachedFromWindow()`, `onSizeChanged(int,int,int,int)`); args[0] is the view itself.
    fn dispatch(self: &Arc<Self>, method: &str, args: &[Local]) -> JResult<Reply> {
        match method {
            "onDraw(Canvas)" => {
                let canvas = args.get(1).and_then(jx::global).ok_or_else(|| JavaError::new("java.lang.NullPointerException", "canvas"))?;
                jcall!(args[0], "com/notemaker/NativeView", "superOnDraw", "(Landroid/graphics/Canvas;)V", &canvas)?;
                self.on_draw(&canvas)?;
                Ok(Reply::Void)
            }
            "onTouchEvent(MotionEvent)" => {
                let event = args.get(1).and_then(jx::global).ok_or_else(|| JavaError::new("java.lang.NullPointerException", "event"))?;
                Ok(Reply::Bool(self.on_touch_event(&event)?))
            }
            "onDetachedFromWindow()" => {
                self.on_detached_from_window()?;
                Ok(Reply::Void)
            }
            "onSizeChanged(int,int,int,int)" => {
                let w = args.get(1).map(|a| jx::unbox_int(a)).unwrap_or(0);
                let h = args.get(2).map(|a| jx::unbox_int(a)).unwrap_or(0);
                self.view_width.store(w, Ordering::SeqCst);
                self.view_height.store(h, Ordering::SeqCst);
                Ok(Reply::Void)
            }
            _ => Ok(Reply::Void),
        }
    }
    /// `CanvasView$1.onScale(ScaleGestureDetector d)`.
    pub fn on_scale(&self, detector: &Obj) -> AppResult<bool> {
        let old = self.zoom.get();
        let factor = jcall!(detector, "android/view/ScaleGestureDetector", "getScaleFactor", "()F")?.float();
        let zoom = fmax(0.2, fmin(5.0, old * factor));
        self.zoom.set(zoom);
        let focus_x = jcall!(detector, "android/view/ScaleGestureDetector", "getFocusX", "()F")?.float();
        self.offset_x.set(focus_x - (focus_x - self.offset_x.get()) * zoom / old);
        let focus_y = jcall!(detector, "android/view/ScaleGestureDetector", "getFocusY", "()F")?.float();
        self.offset_y.set(focus_y - (focus_y - self.offset_y.get()) * zoom / old);
        self.invalidate()?;
        Ok(true)
    }

    /// `boolean pendingErase(String kind, String id, String layer)`.
    fn pending_erase(&self, kind: &str, id: &str, layer: &str) -> bool {
        self.erase_previews.with(|p| {
            if p.is_empty() {
                return false;
            }
            let page = self.app().page.get();
            let key = format!("{kind}:{id}");
            p.values().any(|preview| preview.layer == layer && preview.page == page && preview.hidden.contains(&key))
        })
    }
    /// `float scale()`.
    pub fn scale(&self) -> ViewScale {
        ViewScale(fmin(self.width().0 as f32 / 1000.0, self.height().0 as f32 / 1400.0))
    }
    /// `float x(float x)`.
    fn x(&self, x: f32) -> f32 {
        (x - self.offset_x.get()) / (self.scale().0 * self.zoom.get())
    }
    /// `float y(float y)`.
    fn y(&self, y: f32) -> f32 {
        (y - self.offset_y.get()) / (self.scale().0 * self.zoom.get())
    }
    /// The document point under a view pixel (`x(sx)`, `y(sy)`) at the
    /// current pan and zoom.
    pub fn to_document(&self, p: ScreenPoint) -> DocPointF {
        DocPointF { x: self.x(p.x), y: self.y(p.y) }
    }
    /// `boolean visible(String layer)`.
    pub fn visible(&self, layer: &ids::LayerId) -> bool {
        let layer: &str = layer;
        let app = self.app();
        let Some(l) = app.layers.with(|l| l.get(layer).cloned()) else { return false };
        !app.model.with(|m| is_false_in(&m.layer_active, layer)) && (l.opt_boolean("force") || !app.hidden.with(|h| h.contains(layer)))
    }
    /// `void drawStroke(Canvas c, JSONObject stroke)`.
    pub fn draw_stroke(&self, c: &Obj, stroke: &JSONObject) -> AppResult<()> {
        let map = parse_map(&stroke.to_string())?;
        draw_with_png(c, |png| render::draw_stroke(&map, png))
    }
    /// `void drawInfiniteTemplate(Canvas c, Map<String,Object> guide)`.
    fn draw_infinite_template(&self, c: &Obj, guide: &Map) -> AppResult<()> {
        let pattern = json::java_string(guide.get("template"));
        let mut spacing = json::as_double(guide.get("spacing"))?;
        let unit = self.scale().0 * self.zoom.get();
        if unit <= 0.0 {
            return Ok(());
        }
        let bounds = jcall!(c, "android/graphics/Canvas", "getClipBounds", "()Landroid/graphics/Rect;")?.some();
        let left_i = jget!(bounds, "android/graphics/Rect", "left", "I")?.int();
        let top_i = jget!(bounds, "android/graphics/Rect", "top", "I")?.int();
        let right_i = jget!(bounds, "android/graphics/Rect", "right", "I")?.int();
        let bottom_i = jget!(bounds, "android/graphics/Rect", "bottom", "I")?.int();
        if left_i >= right_i || top_i >= bottom_i {
            return Ok(());
        }
        let mut density_step = jmath::max(1.0, (5.0 / (spacing * unit as f64)).ceil());
        let area = (right_i.wrapping_sub(left_i)) as f64 * (bottom_i.wrapping_sub(top_i)) as f64;
        density_step = jmath::max(density_step, ((area / 3000.0).sqrt() / spacing).ceil());
        spacing *= density_step;
        let left = (left_i as f64 / spacing).floor() * spacing;
        let top = (top_i as f64 / spacing).floor() * spacing;
        let mut color = json::as_int(guide.get("color"))?;
        if color == 0xffc8ccc8u32 as i32 || color == 0xffccccccu32 as i32 {
            color = 0xff777777u32 as i32;
        }
        let paint = &self.paint;
        jcall!(paint, "android/graphics/Paint", "setColor", "(I)V", color)?;
        jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Stroke)?)?;
        jcall!(paint, "android/graphics/Paint", "setStrokeWidth", "(F)V", fmax(1.4f32 / unit, 1.0 / unit))?;
        jcall!(paint, "android/graphics/Paint", "setPathEffect", "(Landroid/graphics/PathEffect;)Landroid/graphics/PathEffect;", jx::NULL)?;
        if pattern == "dots" {
            jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Fill)?)?;
            let radius = fmax(2.1f32 / unit, 1.6f32 / unit);
            let mut y = top;
            while y <= bottom_i as f64 {
                let mut x = left;
                while x <= right_i as f64 {
                    jcall!(c, "android/graphics/Canvas", "drawCircle", "(FFFLandroid/graphics/Paint;)V", x as f32, y as f32, radius, paint)?;
                    x += spacing;
                }
                y += spacing;
            }
        } else {
            let mut y = top;
            while y <= bottom_i as f64 {
                jcall!(c, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", left_i as f32, y as f32, right_i as f32, y as f32, paint)?;
                y += spacing;
            }
            if pattern == "grid" {
                let mut x = left;
                while x <= right_i as f64 {
                    jcall!(c, "android/graphics/Canvas", "drawLine", "(FFFFLandroid/graphics/Paint;)V", x as f32, top_i as f32, x as f32, bottom_i as f32, paint)?;
                    x += spacing;
                }
            }
        }
        jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Fill)?)?;
        Ok(())
    }
    /// `Map<String,Object> tileCacheMetrics()`.
    pub fn tile_cache_metrics(&self) -> Map {
        let (mut m, clears, resident) = self.tile_ink.with(|t| (t.metrics(), t.cache_clears(), t.resident_page_count()));
        m.insert("densityTier".into(), Value::from(self.raster_tier.get().0));
        m.insert("reusedTileRepaints".into(), Value::from(tile_ink_cache::REUSED_TILE_REPAINTS.load(Ordering::SeqCst)));
        m.insert("cacheClears".into(), Value::from(clears.0));
        m.insert("residentPages".into(), Value::from(resident.0));
        m
    }
    /// `void transform(Canvas c)`.
    pub fn transform(&self, c: &Obj) -> AppResult<()> {
        jcall!(c, "android/graphics/Canvas", "translate", "(FF)V", self.offset_x.get(), self.offset_y.get())?;
        let s = self.scale().0 * self.zoom.get();
        jcall!(c, "android/graphics/Canvas", "scale", "(FF)V", s, s)?;
        if self.app().type_.get() != "infinite" {
            jcall!(c, "android/graphics/Canvas", "clipRect", "(IIII)Z", 0, 0, 1000, 1400)?;
        }
        Ok(())
    }
    /// `RectF documentViewport()`.
    pub fn document_viewport(&self) -> RectF {
        let (w, h) = (self.width().0 as f32, self.height().0 as f32);
        let mut viewport = RectF { left: fmin(self.x(0.0), self.x(w)), top: fmin(self.y(0.0), self.y(h)), right: fmax(self.x(0.0), self.x(w)), bottom: fmax(self.y(0.0), self.y(h)) };
        if self.app().type_.get() != "infinite" && !viewport.intersect(0.0, 0.0, 1000.0, 1400.0) {
            viewport.set_empty();
        }
        viewport
    }
    /// `void settledInk(Canvas c, String layer)`.
    fn settled_ink(&self, c: &Obj, layer: &str) -> AppResult<()> {
        let app = self.app();
        let model = app.model();
        let page = app.page.get();
        let trace = app.trace_cold_draw.get();
        let epoch = app.startup_epoch.get();
        let mut visible = tile_ink_cache::LayerItems::default();
        let mut preview_ids: Vec<String> = Vec::new();
        MainActivity::startup_mark(trace, "first_strokes_start", epoch);
        let current = self.current.get();
        app.active.with(|active| -> AppResult<()> {
            for (key, stroke) in &model.strokes {
                if current.as_deref() != Some(key.as_str()) && same_str(stroke, "layer", layer) && json::as_int(stroke.get("page"))? == page && active.get(key) != Some(&false) && !self.pending_erase("stroke", key, layer) {
                    let cached = self.stroke_tile_items.with(|m| m.get(key).filter(|c| Arc::ptr_eq(&c.value, stroke)).cloned());
                    let cached = match cached {
                        Some(c) => c,
                        None => {
                            let c = Arc::new(tile_ink_cache::Item::new(tile_ink_cache::TileItemId::from(key), stroke.clone())?);
                            self.stroke_tile_items.with_mut(|m| m.insert(key.clone(), c.clone()));
                            c
                        }
                    };
                    visible.0.push(cached);
                }
            }
            Ok(())
        })?;
        MainActivity::startup_mark(trace, "first_strokes_prepared", epoch);
        // Live ink uses the same transparent layer, item order and compositing
        // as committed ink. Switching to saved state must not change pixels.
        if layer == app.selected.get() && self.points.with(Option::is_some) {
            if let Some(current) = &current {
                visible.0.push(Arc::new(tile_ink_cache::Item::with_picture(tile_ink_cache::TileItemId::from(current), Arc::new(self.stroke_value()), tile_ink_cache::PictureCache::Transient)?));
            }
        }
        let player = app.animation_player();
        for (key, o) in &model.objects {
            if !same_str(o, "shape", "template") && !player.loaded(&ids::EntityId::from(key)) && same_str(o, "layer", layer) && json::as_int(o.get("page"))? == page && !is_false_in(&model.object_active, key) && !self.pending_erase("object", key, layer) {
                let cached = self.object_tile_items.with(|m| m.get(key).cloned());
                let linked = o.contains_key("from") || o.contains_key("to");
                let same_source = self.object_tile_sources.with(|m| m.get(key).is_some_and(|s| Arc::ptr_eq(s, o)));
                let cached = match cached {
                    Some(cached) if same_source => {
                        if linked {
                            let geometry = DrawItem::new(&model, o.clone())?.geometry().clone();
                            if *cached.value != *geometry {
                                let c = Arc::new(tile_ink_cache::Item::new(tile_ink_cache::TileItemId::from(format!("object:{key}")), geometry)?);
                                self.object_tile_items.with_mut(|m| m.insert(key.clone(), c.clone()));
                                c
                            } else {
                                cached
                            }
                        } else {
                            cached
                        }
                    }
                    _ => {
                        let geometry = DrawItem::new(&model, o.clone())?.geometry().clone();
                        let c = Arc::new(tile_ink_cache::Item::new(tile_ink_cache::TileItemId::from(format!("object:{key}")), geometry)?);
                        self.object_tile_sources.with_mut(|m| m.insert(key.clone(), o.clone()));
                        self.object_tile_items.with_mut(|m| m.insert(key.clone(), c.clone()));
                        c
                    }
                };
                visible.0.push(cached);
            }
        }
        let previews: Vec<(String, Entity)> = app.pending_object_previews.with(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
        for (id, preview) in previews {
            if same_str(&preview, "layer", layer) && json::as_int(preview.get("page"))? == page && !model.objects.contains_key(&id) {
                let geometry = DrawItem::new(&model, preview.clone())?.geometry().clone();
                visible.0.push(Arc::new(tile_ink_cache::Item::with_picture(tile_ink_cache::TileItemId::from(format!("object:{id}")), geometry, tile_ink_cache::PictureCache::Transient)?));
                preview_ids.push(id);
            }
        }
        MainActivity::startup_mark(trace, "first_objects_prepared", epoch);
        MainActivity::startup_mark(trace, "first_ink_prepared", epoch);
        let viewport = self.document_viewport();
        if viewport.is_empty() {
            return Ok(());
        }
        jcall!(c, "android/graphics/Canvas", "save", "()I")?;
        self.transform(c)?;
        let tier = self.raster_tier.get();
        let mut renderer = |image: &Obj, item: &Entity| -> AppResult<()> { draw_with_png(image, |png| render::draw_item(&model, item, png)) };
        match self.monolithic_tile_probe.get() {
            Some(probe) => self.tile_ink.with_mut(|t| t.draw_monolithic(c, &visible, &probe, tier, &mut renderer))?,
            None => {
                let cull = if self.cull_tile_items.get() { tile_ink_cache::ItemCulling::Cull } else { tile_ink_cache::ItemCulling::All };
                self.tile_ink.with_mut(|t| t.draw(c, ids::PageNumber(page), &ids::LayerId::from(layer), &visible, &viewport, tier, cull, &mut renderer))?
            }
        }
        jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
        app.drawn_object_previews.with_mut(|d| d.extend(preview_ids.iter().cloned()));
        app.tiled_object_previews.with_mut(|d| d.extend(preview_ids));
        MainActivity::startup_mark(trace, "first_ink_drawn", epoch);
        Ok(())
    }
    /// `protected void onDraw(Canvas c)` (`super.onDraw` then `synchronized(lock)`).
    pub fn on_draw(&self, c: &Obj) -> AppResult<()> {
        let app = self.app();
        let _sync = app.lock.enter();
        let epoch = app.startup_epoch.get();
        MainActivity::startup_mark(app.trace_cold_draw.get(), "first_canvas_start", epoch);
        let model = app.model();
        let paint = &self.paint;
        if model.deleted {
            jcall!(c, "android/graphics/Canvas", "drawColor", "(I)V", COLOR_WHITE)?;
            jcall!(paint, "android/graphics/Paint", "setColor", "(I)V", COLOR_BLACK)?;
            jcall!(paint, "android/graphics/Paint", "setTextSize", "(F)V", 32.0f32)?;
            jcall!(c, "android/graphics/Canvas", "drawText", "(Ljava/lang/String;FFLandroid/graphics/Paint;)V", "Document deleted · open another file from Notes", 24.0f32, 60.0f32, paint)?;
            return Ok(());
        }
        let erase_timings = app.erase_timings.get();
        trace_put(&erase_timings, "draw_start");
        if self.width().0 == 0 || self.height().0 == 0 {
            return Ok(());
        }
        let player = app.animation_player();
        player.prepare_visible()?;
        let trace = app.trace_cold_draw.get();
        if trace {
            tile_ink_cache::TRACE_FIRST_DRAW.store(true, Ordering::SeqCst);
            png::TRACE_SVG.store(true, Ordering::SeqCst);
            for counter in [&tile_ink_cache::TRACED_COMPILE_NS, &tile_ink_cache::TRACED_PLAY_NS, &tile_ink_cache::TRACED_OTHER_NS, &tile_ink_cache::TRACED_OTHER_MAX_NS] {
                counter.store(0, Ordering::SeqCst);
            }
            for counter in [&tile_ink_cache::TRACED_COMPILES, &tile_ink_cache::TRACED_PLAYS, &tile_ink_cache::TRACED_OTHER_RENDERS] {
                counter.store(0, Ordering::SeqCst);
            }
            *tile_ink_cache::TRACED_OTHER_MAX_SHAPE.lock().unwrap_or_else(|e| e.into_inner()) = Some(String::new());
        }
        self.tile_ink.with_mut(TileInkCache::begin_frame);
        MainActivity::startup_mark(trace, "first_tile_begin", epoch);
        let document = app.document.get();
        let type_ = app.type_.get();
        let page = app.page.get();
        if document != self.ink_document.get() || type_ != self.ink_type.get() {
            self.tile_ink.with_mut(TileInkCache::clear)?;
            self.stroke_tile_items.with_mut(HashMap::clear);
            self.object_tile_sources.with_mut(HashMap::clear);
            self.object_tile_items.with_mut(HashMap::clear);
            self.ink_document.set(document);
            self.ink_type.set(type_.clone());
        }
        if page != self.ink_page.get() {
            self.stroke_tile_items.with_mut(HashMap::clear);
            self.object_tile_sources.with_mut(HashMap::clear);
            self.object_tile_items.with_mut(HashMap::clear);
            self.ink_page.set(page);
        }
        let generation = app.model_generation.get();
        if self.pruned_generation.get() != generation {
            self.stroke_tile_items.with_mut(|m| m.retain(|k, _| model.strokes.contains_key(k)));
            self.object_tile_sources.with_mut(|m| m.retain(|k, _| model.objects.contains_key(k)));
            self.object_tile_items.with_mut(|m| m.retain(|k, _| model.objects.contains_key(k)));
            self.pruned_generation.set(generation);
        }
        self.raster_tier.set(TileInkCache::density_tier(ViewScale(self.scale().0 * self.zoom.get())));
        MainActivity::startup_mark(trace, "first_tile_config", epoch);
        let infinite = type_ == "infinite";
        jcall!(c, "android/graphics/Canvas", "save", "()I")?;
        self.transform(c)?;
        if !infinite {
            jcall!(paint, "android/graphics/Paint", "setColor", "(I)V", 0xffaaaaaau32 as i32)?;
            jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Stroke)?)?;
            jcall!(paint, "android/graphics/Paint", "setStrokeWidth", "(F)V", 1.0f32)?;
            jcall!(c, "android/graphics/Canvas", "drawRect", "(FFFFLandroid/graphics/Paint;)V", 0.0f32, 0.0f32, 1000.0f32, 1400.0f32, paint)?;
            jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Fill)?)?;
        }
        jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
        MainActivity::startup_mark(trace, "first_page_border", epoch);
        let previewing = {
            let previews: Vec<Value> = app.transform_previews.with(|p| p.values().cloned().collect());
            previews.iter().any(|raw| matches!(expires(raw), Ok(e) if e > jx::current_time_millis().0))
        };
        let preview_viewport = if previewing { Some(self.document_viewport()) } else { None };
        let probe = self.monolithic_tile_probe.get();
        let mono = probe.is_some() && !previewing;
        if let (true, Some(probe)) = (mono, probe) {
            jcall!(c, "android/graphics/Canvas", "save", "()I")?;
            let before = jcall!(c, "android/graphics/Canvas", "getMatrix", "()Landroid/graphics/Matrix;")?.some();
            self.transform(c)?;
            let r = android_canvas::rect_to_java(&probe)?;
            jcall!(c, "android/graphics/Canvas", "clipRect", "(Landroid/graphics/RectF;)Z", &r)?;
            jcall!(c, "android/graphics/Canvas", "setMatrix", "(Landroid/graphics/Matrix;)V", &before)?;
            jcall!(c, "android/graphics/Canvas", "drawColor", "(I)V", COLOR_WHITE)?;
        }
        MainActivity::startup_mark(trace, "first_layers_start", epoch);
        let use_ink_cache = self.use_ink_cache.get();
        let selected = app.selected.get();
        let layer_ids: Vec<String> = app.layers.with(|l| l.keys().cloned().collect());
        for layer in &layer_ids {
            if !self.visible(&ids::LayerId::from(layer)) {
                continue;
            }
            MainActivity::startup_mark(trace, "first_layer_visible", epoch);
            jcall!(c, "android/graphics/Canvas", "save", "()I")?;
            MainActivity::startup_mark(trace, "first_layer_saved", epoch);
            let before = jcall!(c, "android/graphics/Canvas", "getMatrix", "()Landroid/graphics/Matrix;")?.some();
            self.transform(c)?;
            let cuts: Vec<Obj> = self.erase_previews.with(|p| p.values().filter(|e| e.layer == *layer && e.page == page).flat_map(|e| e.paths.iter().cloned()).collect());
            for cut in &cuts {
                jcall!(c, "android/graphics/Canvas", "clipOutPath", "(Landroid/graphics/Path;)Z", cut)?;
            }
            jcall!(c, "android/graphics/Canvas", "setMatrix", "(Landroid/graphics/Matrix;)V", &before)?;
            MainActivity::startup_mark(trace, "first_layer_clip_done", epoch);
            if use_ink_cache && !previewing {
                self.settled_ink(c, layer)?;
            } else {
                jcall!(c, "android/graphics/Canvas", "save", "()I")?;
                self.transform(c)?;
                let current = self.current.get();
                let active = app.active.get();
                let strokes: Vec<(String, JSONObject)> = app.strokes.with(|s| s.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
                for (key, s) in &strokes {
                    if current.as_deref() != Some(key.as_str()) && *layer == s.opt_string("layer") && s.opt_int("page") == page && active.get(key) != Some(&false) && !self.pending_erase("stroke", key, layer) && !(previewing && self.previewed("stroke", key)) {
                        if let (true, Some(pv)) = (previewing, &preview_viewport) {
                            let item = parse_map(&s.to_string())?;
                            if !RectF::intersects(pv, &tile_ink_cache::Item::new(tile_ink_cache::TileItemId::from(key), Arc::new(item))?.bounds) {
                                continue;
                            }
                        }
                        self.draw_stroke(c, s)?;
                    }
                }
                jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
            }
            jcall!(c, "android/graphics/Canvas", "save", "()I")?;
            self.transform(c)?;
            if infinite {
                for guide in model.objects.values() {
                    if same_str(guide, "layer", layer) && same_str(guide, "shape", "template") && json::as_int(guide.get("page"))? == page && !json::key_of(guide.get("object")).is_some_and(|k| is_false_in(&model.object_active, k)) && !self.pending_erase("object", &json::java_string(guide.get("object")), layer) {
                        self.draw_infinite_template(c, guide)?;
                    }
                }
            }
            if (!use_ink_cache || previewing) && *layer == selected && self.points.with(Option::is_some) && self.current.with(Option::is_some) {
                self.draw_stroke(c, &self.stroke())?;
            }
            if !use_ink_cache || previewing {
                for (key, o) in &model.objects {
                    if same_str(o, "layer", layer) && !player.loaded(&ids::EntityId::from(key)) && json::as_int(o.get("page"))? == page && !is_false_in(&model.object_active, key) && !self.pending_erase("object", key, layer) && !(previewing && self.previewed("object", key)) {
                        if let (true, Some(pv)) = (previewing, &preview_viewport) {
                            let geometry = DrawItem::new(&model, o.clone())?.geometry().clone();
                            if !RectF::intersects(pv, &tile_ink_cache::Item::new(tile_ink_cache::TileItemId::from(format!("object:{key}")), geometry)?.bounds) {
                                continue;
                            }
                        }
                        draw_with_png(c, |png| render::draw_object(&model, o, png))?;
                    }
                }
                let pending: Vec<(String, Entity)> = app.pending_object_previews.with(|p| p.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
                for (key, preview) in pending {
                    if same_str(&preview, "layer", layer) && json::as_int(preview.get("page"))? == page && !model.objects.contains_key(&key) {
                        draw_with_png(c, |png| render::draw_object(&model, &preview, png))?;
                        app.drawn_object_previews.with_mut(|d| d.insert(key));
                    }
                }
            }
            player.draw(c, &ids::LayerId::from(layer))?;
            jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
            jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
        }
        if mono {
            jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
        }
        jcall!(c, "android/graphics/Canvas", "save", "()I")?;
        self.transform(c)?;
        player.draw_controls(c)?;
        jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
        self.draw_transform_previews(c)?;
        self.draw_selection(c)?;
        if self.lassoing.get() {
            jcall!(c, "android/graphics/Canvas", "save", "()I")?;
            self.transform(c)?;
            jcall!(paint, "android/graphics/Paint", "setColor", "(I)V", COLOR_BLACK)?;
            jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Stroke)?)?;
            jcall!(paint, "android/graphics/Paint", "setStrokeWidth", "(F)V", 2.0 / fmax(0.2, self.zoom.get()))?;
            let dash = jnew!("android/graphics/DashPathEffect", "([FF)V", &jx::new_float_array(&[7.0, 5.0])?, 0.0f32)?;
            jcall!(paint, "android/graphics/Paint", "setPathEffect", "(Landroid/graphics/PathEffect;)Landroid/graphics/PathEffect;", &dash)?;
            let path = self.lasso_path.get();
            if app.freeform_lasso.get() && path.len() > 1 {
                let outline = jnew!("android/graphics/Path", "()V")?;
                jcall!(outline, "android/graphics/Path", "moveTo", "(FF)V", path[0][0], path[0][1])?;
                for p in &path[1..] {
                    jcall!(outline, "android/graphics/Path", "lineTo", "(FF)V", p[0], p[1])?;
                }
                jcall!(c, "android/graphics/Canvas", "drawPath", "(Landroid/graphics/Path;Landroid/graphics/Paint;)V", &outline, paint)?;
            } else {
                jcall!(c, "android/graphics/Canvas", "drawRect", "(FFFFLandroid/graphics/Paint;)V", self.lasso_start_x.get(), self.lasso_start_y.get(), self.lasso_end_x.get(), self.lasso_end_y.get(), paint)?;
            }
            jcall!(paint, "android/graphics/Paint", "setPathEffect", "(Landroid/graphics/PathEffect;)Landroid/graphics/PathEffect;", jx::NULL)?;
            jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Fill)?)?;
            jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
        }
        let input_started = self.input_started.get();
        if input_started != 0 {
            if let Some(latency) = self.draw_latency.get() {
                latency(jx::nano_time().0 - input_started);
            }
            self.input_started.set(0);
        }
        let drawing_shape = app.drawing_shape.get();
        if let (true, Some(shape)) = (self.shape_dragging.get(), drawing_shape.as_deref()) {
            if shape != "text" {
                let (sx, sy, ex, ey) = (self.shape_start_x.get(), self.shape_start_y.get(), self.shape_end_x.get(), self.shape_end_y.get());
                let (dx, dy) = (ex - sx, ey - sy);
                let (mut left, mut top, mut w, mut h) = (fmin(sx, ex), fmin(sy, ey), dx.abs(), dy.abs());
                if shape == "line" || shape == "arrow" {
                    left = sx;
                    top = sy;
                    w = dx;
                    h = dy;
                    if w.abs() + h.abs() < 1.0 {
                        w = 1.0;
                        h = 1.0;
                    }
                } else {
                    w = fmax(12.0, w);
                    h = fmax(12.0, h);
                    if !infinite {
                        left = fmax(0.0, fmin(1000.0 - w, left));
                        top = fmax(0.0, fmin(1400.0 - h, top));
                    }
                }
                let preview = app.object_value(shape, left, top, w, h, app.color.get(), app.width.get(), app.object_fill_color.get());
                let preview = parse_map(&preview.to_string())?;
                jcall!(c, "android/graphics/Canvas", "save", "()I")?;
                self.transform(c)?;
                draw_with_png(c, |png| render::draw_object(&model, &preview, png))?;
                jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
            }
        }
        trace_put(&erase_timings, "draw_done");
        if let Some(after) = self.after_draw.get() {
            after();
        }
        MainActivity::startup_mark(app.trace_cold_draw.get(), "first_canvas_done", epoch);
        app.trace_cold_draw.set(false);
        if tile_ink_cache::TRACE_FIRST_DRAW.load(Ordering::SeqCst) {
            let ms = |v: &std::sync::atomic::AtomicI64| jfmt::double(v.load(Ordering::SeqCst) as f64 / 1e6);
            let shape = tile_ink_cache::TRACED_OTHER_MAX_SHAPE.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| "null".into());
            jx::log::i(
                crate::jx::log::STARTUP,
                &format!(
                    "tile_compile_ms={} tile_play_ms={} tile_other_ms={} other_max_ms={} other_shape={} compiles={} plays={} other_renders={}",
                    ms(&tile_ink_cache::TRACED_COMPILE_NS),
                    ms(&tile_ink_cache::TRACED_PLAY_NS),
                    ms(&tile_ink_cache::TRACED_OTHER_NS),
                    ms(&tile_ink_cache::TRACED_OTHER_MAX_NS),
                    shape,
                    tile_ink_cache::TRACED_COMPILES.load(Ordering::SeqCst),
                    tile_ink_cache::TRACED_PLAYS.load(Ordering::SeqCst),
                    tile_ink_cache::TRACED_OTHER_RENDERS.load(Ordering::SeqCst)
                ),
            );
            tile_ink_cache::TRACE_FIRST_DRAW.store(false, Ordering::SeqCst);
            png::TRACE_SVG.store(false, Ordering::SeqCst);
        }
        Ok(())
    }
    /// `protected void onDetachedFromWindow()` (the shim calls `super` afterwards).
    pub fn on_detached_from_window(&self) -> AppResult<()> {
        self.tile_ink.with_mut(TileInkCache::clear)?;
        self.stroke_tile_items.with_mut(HashMap::clear);
        self.object_tile_sources.with_mut(HashMap::clear);
        self.object_tile_items.with_mut(HashMap::clear);
        Ok(())
    }
    /// `JSONObject stroke()`.
    pub fn stroke(&self) -> JSONObject {
        let app = self.app();
        let o = MainActivity::obj(&[("kind", JV::from("stroke"))]);
        // `obj(..., "stroke", current, ..., "points", points)`: a Java null removes the key.
        let put = (|| -> Result<(), org_json::JSONException> {
            o.put_opt("stroke", self.current.get().map(JV::from))?;
            o.put("layer", app.selected.get())?;
            o.put("page", JV::Int(app.page.get()))?;
            o.put("color", JV::Int(app.color.get()))?;
            o.put("width", JV::Float(app.width.get()))?;
            o.put("brush", app.brush.get())?;
            o.put("material", JV::Int(app.material.get()))?;
            o.put_opt("points", self.points.get().map(JV::Arr))?;
            Ok(())
        })();
        if let Err(e) = put {
            panic!("{}", AppError::runtime_cause(&AppError::from(e)));
        }
        o
    }
    /// `Map<String,Object> strokeValue()`.
    fn stroke_value(&self) -> Map {
        let app = self.app();
        let coordinates = match self.points.get() {
            None => Vec::new(),
            Some(points) => {
                let cached = self.point_values.get();
                if cached.len() == points.length() {
                    cached
                } else {
                    points.values().iter().map(Self::point_value).collect()
                }
            }
        };
        object! {
            "kind" => "stroke",
            "stroke" => self.current.get().map(Value::String).unwrap_or(Value::Null),
            "layer" => app.selected.get(),
            "page" => app.page.get(),
            "color" => app.color.get(),
            "width" => json::from_float(app.width.get()),
            "brush" => app.brush.get(),
            "material" => app.material.get(),
            "points" => coordinates,
        }
    }
    /// `boolean previewed(String kind, String id)`.
    fn previewed(&self, kind: &str, id: &str) -> bool {
        let now = jx::current_time_millis().0;
        let previews: Vec<Value> = self.app().transform_previews.with(|p| p.values().cloned().collect());
        for raw in previews {
            let hit = (|| -> AppResult<bool> {
                let preview = json::map(&raw)?;
                if long_or(preview, "expires", 0)? <= now {
                    return Ok(false);
                }
                for item in json::list(preview.get("entities").unwrap_or(&Value::Null))? {
                    let entity = json::map(item)?;
                    if same_str(entity, "kind", kind) && same_str(entity, "id", id) {
                        return Ok(true);
                    }
                }
                Ok(false)
            })();
            if let Ok(true) = hit {
                return true;
            }
        }
        false
    }
    /// `RectF inversePreviewViewport(RectF viewport, double px, double py, double dx, double dy, double factor, double rotation, boolean flipX, boolean flipY)`.
    fn inverse_preview_viewport(&self, viewport: &RectF, px: f64, py: f64, dx: f64, dy: f64, factor: f64, rotation: f64, flip_x: bool, flip_y: bool) -> RectF {
        let a = jmath::to_radians(rotation);
        let (cos, sin) = (jmath::cos(a), jmath::sin(a));
        let sx = factor * if flip_x { -1.0 } else { 1.0 };
        let sy = factor * if flip_y { -1.0 } else { 1.0 };
        let p = [viewport.left, viewport.top, viewport.right, viewport.top, viewport.right, viewport.bottom, viewport.left, viewport.bottom];
        let (mut l, mut t, mut r, mut b) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for i in (0..p.len()).step_by(2) {
            let vx = p[i] as f64 - (px + dx);
            let vy = p[i + 1] as f64 - (py + dy);
            let rx = (cos * vx + sin * vy) / sx + px;
            let ry = (-sin * vx + cos * vy) / sy + py;
            l = jmath::min(l, rx);
            r = jmath::max(r, rx);
            t = jmath::min(t, ry);
            b = jmath::max(b, ry);
        }
        RectF { left: l as f32, top: t as f32, right: r as f32, bottom: b as f32 }
    }
    /// `void drawTransformPreviews(Canvas c)`.
    pub fn draw_transform_previews(&self, c: &Obj) -> AppResult<()> {
        let app = self.app();
        let now = jx::current_time_millis().0;
        let viewport = self.document_viewport();
        if viewport.is_empty() {
            return Ok(());
        }
        let pending_transform = app.pending_transform.get();
        let mut previews: Vec<Value> = Vec::new();
        let shared: Vec<Value> = app.transform_previews.with(|p| p.values().cloned().collect());
        for raw in shared {
            if let Some(pending) = &pending_transform {
                if json::objects_equal(json::map(&raw)?.get("session"), pending.get("session")) {
                    continue;
                }
            }
            previews.push(raw);
        }
        if let Some(pending) = &pending_transform {
            let mut local = pending.clone();
            local.insert("expires".into(), Value::from(now + 250));
            previews.push(Value::Object(local));
        }
        let model = app.model();
        for raw in previews {
            let _ = (|| -> AppResult<()> {
                let preview = json::map(&raw)?;
                if long_or(preview, "expires", 0)? <= now {
                    return Ok(());
                }
                let tr = json::map(preview.get("transform").unwrap_or(&Value::Null))?;
                let factor = double_or(tr, "scale", 1.0)?;
                let dx = double_or(tr, "dx", 0.0)?;
                let dy = double_or(tr, "dy", 0.0)?;
                let rotation = double_or(tr, "rotation", 0.0)?;
                let flip_x = json::is_true(tr.get("flipX"));
                let flip_y = json::is_true(tr.get("flipY"));
                let entities = json::list(preview.get("entities").unwrap_or(&Value::Null))?;
                let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                for raw_entity in entities {
                    let entity = json::map(raw_entity)?;
                    let kind = json::java_string(entity.get("kind"));
                    let id = json::java_string(entity.get("id"));
                    if kind == "object" {
                        if let Some(o) = model.objects.get(&id) {
                            let x0 = json::as_double(o.get("x"))?;
                            let y0 = json::as_double(o.get("y"))?;
                            min_x = jmath::min(min_x, x0);
                            max_x = jmath::max(max_x, x0 + json::as_double(o.get("w"))?);
                            min_y = jmath::min(min_y, y0);
                            max_y = jmath::max(max_y, y0 + json::as_double(o.get("h"))?);
                        }
                    } else if let Some(s) = app.strokes.with(|s| s.get(&id).cloned()) {
                        let ps = s.opt_json_array("points").ok_or_else(npe)?;
                        for i in 0..ps.length() {
                            let p = ps.opt_json_array(i).ok_or_else(npe)?;
                            min_x = jmath::min(min_x, p.opt_double(0));
                            max_x = jmath::max(max_x, p.opt_double(0));
                            min_y = jmath::min(min_y, p.opt_double(1));
                            max_y = jmath::max(max_y, p.opt_double(1));
                        }
                    }
                }
                if min_x == f64::INFINITY {
                    return Ok(());
                }
                let px = double_or(tr, "pivotX", (min_x + max_x) / 2.0)?;
                let py = double_or(tr, "pivotY", (min_y + max_y) / 2.0)?;
                let source_viewport = self.inverse_preview_viewport(&viewport, px, py, dx, dy, factor, rotation, flip_x, flip_y);
                let layer_save = jcall!(c, "android/graphics/Canvas", "saveLayerAlpha", "(Landroid/graphics/RectF;I)I", jx::NULL, 96)?.int();
                jcall!(c, "android/graphics/Canvas", "save", "()I")?;
                self.transform(c)?;
                jcall!(c, "android/graphics/Canvas", "translate", "(FF)V", (px + dx) as f32, (py + dy) as f32)?;
                jcall!(c, "android/graphics/Canvas", "rotate", "(F)V", rotation as f32)?;
                jcall!(c, "android/graphics/Canvas", "scale", "(FF)V", (factor * if flip_x { -1.0 } else { 1.0 }) as f32, (factor * if flip_y { -1.0 } else { 1.0 }) as f32)?;
                jcall!(c, "android/graphics/Canvas", "translate", "(FF)V", -px as f32, -py as f32)?;
                for raw_entity in entities {
                    let entity = json::map(raw_entity)?;
                    let kind = json::java_string(entity.get("kind"));
                    let id = json::java_string(entity.get("id"));
                    if kind == "object" {
                        if let Some(o) = model.objects.get(&id) {
                            let geometry = DrawItem::new(&model, o.clone())?.geometry().clone();
                            if RectF::intersects(&source_viewport, &tile_ink_cache::Item::new(tile_ink_cache::TileItemId::from(format!("object:{id}")), geometry)?.bounds) {
                                draw_with_png(c, |png| render::draw_object(&model, o, png))?;
                            }
                        }
                    } else if kind == "stroke" {
                        if let Some(s) = app.strokes.with(|s| s.get(&id).cloned()) {
                            let item = parse_map(&s.to_string())?;
                            if RectF::intersects(&source_viewport, &tile_ink_cache::Item::new(tile_ink_cache::TileItemId::from(&id), Arc::new(item))?.bounds) {
                                self.draw_stroke(c, &s)?;
                            }
                        }
                    }
                }
                jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
                jcall!(c, "android/graphics/Canvas", "restoreToCount", "(I)V", layer_save)?;
                Ok(())
            })();
        }
        Ok(())
    }
    /// `void drawSelection(Canvas c)`.
    pub fn draw_selection(&self, c: &Obj) -> AppResult<()> {
        let app = self.app();
        let selected = app.selected_entities.get();
        if selected.is_empty() {
            return Ok(());
        }
        let paint = &self.paint;
        jcall!(c, "android/graphics/Canvas", "save", "()I")?;
        self.transform(c)?;
        jcall!(paint, "android/graphics/Paint", "setColor", "(I)V", COLOR_BLACK)?;
        jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Stroke)?)?;
        jcall!(paint, "android/graphics/Paint", "setStrokeWidth", "(F)V", 2.0 / fmax(0.2, self.zoom.get()))?;
        let dash = jnew!("android/graphics/DashPathEffect", "([FF)V", &jx::new_float_array(&[8.0, 5.0])?, 0.0f32)?;
        jcall!(paint, "android/graphics/Paint", "setPathEffect", "(Landroid/graphics/PathEffect;)Landroid/graphics/PathEffect;", &dash)?;
        let model = app.model();
        for entity in &selected {
            let kind = json::java_string(entity.get("kind"));
            let id = json::java_string(entity.get("id"));
            if kind == "object" {
                if let Some(o) = model.objects.get(&id) {
                    let x = json::as_double(o.get("x"))? as f32;
                    let y = json::as_double(o.get("y"))? as f32;
                    let w = json::as_double(o.get("w"))? as f32;
                    let h = json::as_double(o.get("h"))? as f32;
                    jcall!(c, "android/graphics/Canvas", "save", "()I")?;
                    jcall!(c, "android/graphics/Canvas", "translate", "(FF)V", x + w / 2.0, y + h / 2.0)?;
                    jcall!(c, "android/graphics/Canvas", "rotate", "(F)V", double_or(o, "rotation", 0.0)? as f32)?;
                    jcall!(c, "android/graphics/Canvas", "scale", "(FF)V", if json::is_true(o.get("flipX")) { -1.0f32 } else { 1.0 }, if json::is_true(o.get("flipY")) { -1.0f32 } else { 1.0 })?;
                    jcall!(c, "android/graphics/Canvas", "drawRect", "(FFFFLandroid/graphics/Paint;)V", -w / 2.0 - 4.0, -h / 2.0 - 4.0, w / 2.0 + 4.0, h / 2.0 + 4.0, paint)?;
                    jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
                }
            } else if let Some(s) = app.strokes.with(|s| s.get(&id).cloned()) {
                let ps = s.opt_json_array("points").ok_or_else(npe)?;
                let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, -f32::MAX, -f32::MAX);
                for i in 0..ps.length() {
                    let p = ps.opt_json_array(i).ok_or_else(npe)?;
                    l = fmin(l, p.opt_double(0) as f32);
                    r = fmax(r, p.opt_double(0) as f32);
                    t = fmin(t, p.opt_double(1) as f32);
                    b = fmax(b, p.opt_double(1) as f32);
                }
                jcall!(c, "android/graphics/Canvas", "drawRect", "(FFFFLandroid/graphics/Paint;)V", l - 8.0, t - 8.0, r + 8.0, b + 8.0, paint)?;
            }
        }
        jcall!(paint, "android/graphics/Paint", "setPathEffect", "(Landroid/graphics/PathEffect;)Landroid/graphics/PathEffect;", jx::NULL)?;
        jcall!(paint, "android/graphics/Paint", "setStyle", "(Landroid/graphics/Paint$Style;)V", &style(Style::Fill)?)?;
        jcall!(c, "android/graphics/Canvas", "restore", "()V")?;
        Ok(())
    }
    /// `void emit()`.
    pub fn emit(&self) -> AppResult<()> {
        self.emit_final(false)
    }
    /// `void emit(boolean finalStroke)`.
    fn emit_final(&self, final_stroke: bool) -> AppResult<()> {
        if self.points.with(|p| p.as_ref().is_some_and(|p| p.length() > 0)) {
            let app = self.app();
            let snapshot = self.stroke_value();
            if final_stroke && !app.replica().remote() {
                app.enqueue_final_stroke(&snapshot)?;
            } else {
                app.enqueue_with(&json_object(&snapshot), &snapshot)?;
            }
            self.last_sent.set(jx::current_time_millis().0);
        }
        Ok(())
    }
    /// `void point(float sx, float sy, float pressure)`.
    fn point(&self, touch: ScreenPoint, pressure: StylusPressure) -> AppResult<()> {
        let DocPointF { x: dx, y: dy } = self.to_document(touch);
        let pressure = pressure.0;
        if self.app().type_.get() != "infinite" && (dx < 0.0 || dy < 0.0 || dx > 1000.0 || dy > 1400.0) {
            return Ok(());
        }
        let mut points = self.points.get().ok_or_else(npe)?;
        if points.length() >= 8000 {
            let reduced = JSONArray::new();
            for i in (0..points.length()).step_by(2) {
                match points.opt_json_array(i) {
                    Some(p) => reduced.put(p)?,
                    None => reduced.put(JV::Null)?,
                };
            }
            self.set_points(Some(reduced.clone()));
            points = reduced;
        }
        let p = fmax(0.0, fmin(1.0, pressure));
        let point = JSONArray::new();
        point.put(JV::Float(dx))?;
        point.put(JV::Float(dy))?;
        point.put(JV::Float(p))?;
        points.put(point)?;
        let value = Value::Array(vec![json::from_float(dx), json::from_float(dy), json::from_float(p)]);
        self.point_values.with_mut(|v| v.push(value));
        Ok(())
    }
    /// `void erasePoint(float sx, float sy)`.
    fn erase_point(&self, touch: ScreenPoint) -> AppResult<()> {
        let DocPointF { x: dx, y: dy } = self.to_document(touch);
        if self.app().type_.get() == "infinite" || (dx >= 0.0 && dy >= 0.0 && dx <= 1000.0 && dy <= 1400.0) {
            let point = JSONArray::new();
            point.put(JV::Float(dx))?;
            point.put(JV::Float(dy))?;
            self.eraser_points.get().ok_or_else(npe)?.put(point)?;
        }
        Ok(())
    }
    /// `void eraseAt(float sx, float sy)`.
    pub fn erase_at(&self, touch: ScreenPoint) -> AppResult<()> {
        let path = JSONArray::new();
        let point = JSONArray::new();
        let at = self.to_document(touch);
        point.put(JV::Float(at.x))?;
        point.put(JV::Float(at.y))?;
        path.put(point)?;
        let app = self.app();
        app.apply_erase(&EraserMode::from("stroke"), &ids::LayerId::from(app.selected.get()), ids::PageNumber(app.page.get()), Some(&path), geometry::Radius(app.eraser_radius.get() as f64))
    }
    /// `boolean scribble(JSONArray path)`.
    pub fn scribble(&self, path: Option<&JSONArray>) -> bool {
        let Some(path) = path else { return false };
        if path.length() < 7 {
            return false;
        }
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::MAX, f64::MAX, -1.0f64, -1.0f64);
        let mut changes = 0;
        let mut previous = 0;
        let points: Vec<Option<JSONArray>> = (0..path.length()).map(|i| path.opt_json_array(i)).collect();
        let coordinate = |p: &Option<JSONArray>, i: usize| p.as_ref().map_or(f64::NAN, |p| p.opt_double(i));
        for i in 0..points.len() {
            let (px, py) = (coordinate(&points[i], 0), coordinate(&points[i], 1));
            min_x = jmath::min(min_x, px);
            max_x = jmath::max(max_x, px);
            min_y = jmath::min(min_y, py);
            max_y = jmath::max(max_y, py);
            if i > 0 {
                let dx = px - coordinate(&points[i - 1], 0);
                let dy = py - coordinate(&points[i - 1], 1);
                let d = if dx.abs() > dy.abs() { dx } else { dy };
                let sign = if d > 2.0 {
                    1
                } else if d < -2.0 {
                    -1
                } else {
                    0
                };
                if sign != 0 {
                    if previous != 0 && sign != previous {
                        changes += 1;
                    }
                    previous = sign;
                }
            }
        }
        let (span_x, span_y) = (max_x - min_x, max_y - min_y);
        changes >= 3 && jmath::max(span_x, span_y) >= jmath::max(30.0, (self.app().eraser_radius.get() * 2.0) as f64)
    }
    /// `public boolean onTouchEvent(MotionEvent event)`.
    pub fn on_touch_event(&self, event: &Obj) -> AppResult<bool> {
        let app = self.app();
        if app.model.with(|m| m.deleted) {
            return Ok(true);
        }
        if self.input_started.get() == 0 {
            self.input_started.set(jx::nano_time().0);
        }
        let _sync = app.lock.enter();
        let scaling = self.scaling.get().ok_or_else(npe)?;
        jcall!(scaling, "android/view/ScaleGestureDetector", "onTouchEvent", "(Landroid/view/MotionEvent;)Z", event)?;
        let action = jcall!(event, "android/view/MotionEvent", "getActionMasked", "()I")?.int();
        let sx = jcall!(event, "android/view/MotionEvent", "getX", "()F")?.float();
        let sy = jcall!(event, "android/view/MotionEvent", "getY", "()F")?.float();
        let touch = ScreenPoint { x: sx, y: sy };
        let pointer_count = jcall!(event, "android/view/MotionEvent", "getPointerCount", "()I")?.int();
        let editor = app.editor();
        if pointer_count > 1 {
            if editor.selected_tool() == "interact" {
                app.animation_player().touch(MotionAction(MOTIONEVENT_ACTION_CANCEL), DocPointF { x: self.x(sx), y: self.y(sy) })?;
            }
            if action == MOTIONEVENT_ACTION_POINTER_DOWN {
                self.last_gesture_x.set(self.centroid_x(event)?);
                self.last_gesture_y.set(self.centroid_y(event)?);
            }
            if action == MOTIONEVENT_ACTION_MOVE && !jcall!(scaling, "android/view/ScaleGestureDetector", "isInProgress", "()Z")?.bool() {
                let (cx, cy) = (self.centroid_x(event)?, self.centroid_y(event)?);
                self.offset_x.set(self.offset_x.get() + (cx - self.last_gesture_x.get()));
                self.offset_y.set(self.offset_y.get() + (cy - self.last_gesture_y.get()));
                self.last_gesture_x.set(cx);
                self.last_gesture_y.set(cy);
            }
            if self.shape_dragging.get() {
                self.shape_dragging.set(false);
            }
            if self.eraser_points.with(Option::is_some) {
                self.eraser_points.set(None);
            }
            self.set_points(None);
            self.current.set(None);
            self.transforming.set(false);
            self.lassoing.set(false);
            self.invalidate()?;
            return Ok(true);
        }
        let action_index = jcall!(event, "android/view/MotionEvent", "getActionIndex", "()I")?.int();
        let tool_type = jcall!(event, "android/view/MotionEvent", "getToolType", "(I)I", action_index)?.int();
        let stylus = tool_type == MOTIONEVENT_TOOL_TYPE_STYLUS || tool_type == MOTIONEVENT_TOOL_TYPE_ERASER;
        if editor.selected_tool() == "interact" {
            self.input_started.set(0);
            return app.animation_player().touch(MotionAction(action), DocPointF { x: self.x(sx), y: self.y(sy) });
        }
        let history_size = || -> AppResult<i32> { Ok(jcall!(event, "android/view/MotionEvent", "getHistorySize", "()I")?.int()) };
        let historical_x = |i: i32| -> AppResult<f32> { Ok(jcall!(event, "android/view/MotionEvent", "getHistoricalX", "(I)F", i)?.float()) };
        let historical_y = |i: i32| -> AppResult<f32> { Ok(jcall!(event, "android/view/MotionEvent", "getHistoricalY", "(I)F", i)?.float()) };
        let pressure = || -> AppResult<StylusPressure> { Ok(StylusPressure(jcall!(event, "android/view/MotionEvent", "getPressure", "()F")?.float())) };
        let infinite = app.type_.get() == "infinite";
        if app.pan.get() {
            if action == MOTIONEVENT_ACTION_DOWN {
                self.last_x.set(sx);
                self.last_y.set(sy);
            }
            if action == MOTIONEVENT_ACTION_MOVE && !jcall!(scaling, "android/view/ScaleGestureDetector", "isInProgress", "()Z")?.bool() && pointer_count == 1 {
                self.offset_x.set(self.offset_x.get() + (sx - self.last_x.get()));
                self.offset_y.set(self.offset_y.get() + (sy - self.last_y.get()));
                self.invalidate()?;
            }
            self.last_x.set(sx);
            self.last_y.set(sy);
            return Ok(true);
        }
        if action == MOTIONEVENT_ACTION_DOWN {
            let selected = app.selected.get();
            if !self.visible(&ids::LayerId::from(&selected)) {
                app.toast("Show the selected layer to draw on it", TOAST_LENGTH_SHORT)?;
                return Ok(true);
            }
            if app.fill_tap_mode.get() {
                app.fill_tap_mode.set(false);
                app.fill_at(self.to_document(touch))?;
                self.invalidate()?;
                return Ok(true);
            }
            if app.selecting.get() {
                if app.selected_entities.with(Vec::is_empty) {
                    self.lassoing.set(true);
                    let (x, y) = (self.x(sx), self.y(sy));
                    self.lasso_start_x.set(x);
                    self.lasso_end_x.set(x);
                    self.lasso_start_y.set(y);
                    self.lasso_end_y.set(y);
                    self.lasso_path.set(vec![[x, y]]);
                } else {
                    if let Some(hit) = app.selected_entity_at(self.to_document(touch))? {
                        app.deselect_entity(&ids::EntityKind::from(json::java_string(hit.get("kind"))), &ids::EntityId::from(json::java_string(hit.get("id"))))?;
                        return Ok(true);
                    }
                    self.transforming.set(true);
                    self.transform_start_x.set(self.x(sx));
                    self.transform_start_y.set(self.y(sy));
                    self.transform_delta_x.set(0.0);
                    self.transform_delta_y.set(0.0);
                }
                return Ok(true);
            }
            if app.drawing_shape.with(Option::is_some) {
                let (dx, dy) = (self.x(sx), self.y(sy));
                if !infinite && (dx < 0.0 || dy < 0.0 || dx > 1000.0 || dy > 1400.0) {
                    return Ok(true);
                }
                self.shape_start_x.set(dx);
                self.shape_end_x.set(dx);
                self.shape_start_y.set(dy);
                self.shape_end_y.set(dy);
                self.shape_dragging.set(true);
                self.invalidate()?;
                return Ok(true);
            }
            if !stylus {
                return Ok(true);
            }
            if app.erase.get() {
                self.eraser_points.set(Some(JSONArray::new()));
                self.erase_point(touch)?;
                self.invalidate()?;
            } else {
                self.current.set(Some(String::from(MainActivity::id())));
                self.set_points(Some(JSONArray::new()));
                self.point(touch, pressure()?)?;
                if !app.scribble_to_erase.get() {
                    self.emit()?;
                }
            }
        }
        if action == MOTIONEVENT_ACTION_MOVE {
            if self.lassoing.get() {
                self.lasso_end_x.set(self.x(sx));
                self.lasso_end_y.set(self.y(sy));
                if app.freeform_lasso.get() {
                    for i in 0..history_size()? {
                        app.add_lasso_point(self.x(historical_x(i)?), self.y(historical_y(i)?));
                    }
                    app.add_lasso_point(self.lasso_end_x.get(), self.lasso_end_y.get());
                }
                self.invalidate()?;
                return Ok(true);
            }
            if self.transforming.get() {
                self.transform_delta_x.set(self.x(sx) - self.transform_start_x.get());
                self.transform_delta_y.set(self.y(sy) - self.transform_start_y.get());
                if jx::current_time_millis().0 - self.last_preview_at.get() > 100 {
                    app.preview_selection(SelectionTransform { scale: 1.0, dx: self.transform_delta_x.get() as f64, dy: self.transform_delta_y.get() as f64 })?;
                    self.last_preview_at.set(jx::current_time_millis().0);
                }
                self.invalidate()?;
                return Ok(true);
            }
            if self.shape_dragging.get() {
                self.shape_end_x.set(self.x(sx));
                self.shape_end_y.set(self.y(sy));
                if !infinite {
                    self.shape_end_x.set(fmax(0.0, fmin(1000.0, self.shape_end_x.get())));
                    self.shape_end_y.set(fmax(0.0, fmin(1400.0, self.shape_end_y.get())));
                }
                self.invalidate()?;
                return Ok(true);
            }
            if app.erase.get() && self.eraser_points.with(Option::is_some) {
                for i in 0..history_size()? {
                    self.erase_point(ScreenPoint { x: historical_x(i)?, y: historical_y(i)? })?;
                }
                self.erase_point(touch)?;
                self.invalidate()?;
            } else if self.points.with(Option::is_some) {
                for i in 0..history_size()? {
                    let p = jcall!(event, "android/view/MotionEvent", "getHistoricalPressure", "(I)F", i)?.float();
                    self.point(ScreenPoint { x: historical_x(i)?, y: historical_y(i)? }, StylusPressure(p))?;
                }
                self.point(touch, pressure()?)?;
                if !app.scribble_to_erase.get() && jx::current_time_millis().0 - self.last_sent.get() > 100 {
                    self.emit()?;
                }
                self.invalidate()?;
            }
        }
        if action == MOTIONEVENT_ACTION_CANCEL {
            if self.lassoing.get() {
                self.lassoing.set(false);
                self.lasso_path.with_mut(Vec::clear);
                self.invalidate()?;
                return Ok(true);
            }
            if self.transforming.get() {
                self.transforming.set(false);
                app.cancel_transform_preview()?;
            } else if self.shape_dragging.get() {
                self.shape_dragging.set(false);
            } else if self.eraser_points.with(Option::is_some) {
                self.eraser_points.set(None);
            } else if self.points.with(Option::is_some) && !app.scribble_to_erase.get() {
                self.point(touch, pressure()?)?;
                let current = self.current.get().unwrap_or_else(|| "null".into());
                app.undo.with_mut(|u| u.push(current));
                app.redo.with_mut(Vec::clear);
                self.emit_final(true)?;
                if app.replica().remote() {
                    app.save();
                }
            }
            self.set_points(None);
            self.current.set(None);
            self.invalidate()?;
            return Ok(true);
        }
        if action == MOTIONEVENT_ACTION_UP {
            if self.lassoing.get() {
                self.lasso_end_x.set(self.x(sx));
                self.lasso_end_y.set(self.y(sy));
                let freeform = app.freeform_lasso.get();
                if freeform {
                    app.add_lasso_point(self.lasso_end_x.get(), self.lasso_end_y.get());
                }
                let polygon = self.lasso_path.get();
                let (lx, ly, rx, ry) = (self.lasso_start_x.get(), self.lasso_start_y.get(), self.lasso_end_x.get(), self.lasso_end_y.get());
                let drag = if freeform && polygon.len() > 1 {
                    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
                    for p in &polygon {
                        min_x = fmin(min_x, p[0]);
                        min_y = fmin(min_y, p[1]);
                        max_x = fmax(max_x, p[0]);
                        max_y = fmax(max_y, p[1]);
                    }
                    fmax(max_x - min_x, max_y - min_y) > 12.0
                } else {
                    jmath::hypot((rx - lx) as f64, (ry - ly) as f64) > 12.0
                };
                self.lassoing.set(false);
                self.lasso_path.with_mut(Vec::clear);
                if drag {
                    if freeform && polygon.len() >= 3 {
                        app.select_lasso(&polygon)?;
                    } else {
                        app.select_within(lx, ly, rx, ry)?;
                    }
                } else {
                    app.select_at(DocPointF { x: rx, y: ry })?;
                }
                self.invalidate()?;
                return Ok(true);
            }
            if self.transforming.get() {
                self.transform_delta_x.set(self.x(sx) - self.transform_start_x.get());
                self.transform_delta_y.set(self.y(sy) - self.transform_start_y.get());
                self.transforming.set(false);
                let (dx, dy) = (self.transform_delta_x.get(), self.transform_delta_y.get());
                if dx.abs() + dy.abs() > 1.0 {
                    app.transform_selection(SelectionTransform { scale: 1.0, dx: dx as f64, dy: dy as f64 }, TransformCommit::Commit)?;
                } else {
                    app.cancel_transform_preview()?;
                }
                return Ok(true);
            }
            if self.shape_dragging.get() {
                self.shape_end_x.set(self.x(sx));
                self.shape_end_y.set(self.y(sy));
                if !infinite {
                    self.shape_end_x.set(fmax(0.0, fmin(1000.0, self.shape_end_x.get())));
                    self.shape_end_y.set(fmax(0.0, fmin(1400.0, self.shape_end_y.get())));
                }
                self.shape_dragging.set(false);
                let shape = app.drawing_shape.get().ok_or_else(npe)?;
                let (sx0, sy0, ex, ey) = (self.shape_start_x.get(), self.shape_start_y.get(), self.shape_end_x.get(), self.shape_end_y.get());
                if shape == "text" {
                    if let Some(text) = app.pending_text.get() {
                        app.place_text(Some(&notemaker_core::style::TextContent::from(text)), notemaker_core::style::FontSize(app.pending_text_size.get() as f64), DocPointF { x: sx0, y: sy0 })?;
                        app.pending_text.set(None);
                    }
                    app.drawing_shape.set(None);
                } else {
                    let (dx, dy) = (ex - sx0, ey - sy0);
                    let (mut left, mut top, mut w, mut h) = (fmin(sx0, ex), fmin(sy0, ey), dx.abs(), dy.abs());
                    if shape == "line" || shape == "arrow" || shape == "arrowLine" {
                        left = sx0;
                        top = sy0;
                        w = dx;
                        h = dy;
                        if w.abs() + h.abs() < 1.0 {
                            w = 1.0;
                            h = 1.0;
                        }
                    } else {
                        w = fmax(12.0, w);
                        h = fmax(12.0, h);
                        if !infinite {
                            left = fmax(0.0, fmin(1000.0 - w, left));
                            top = fmax(0.0, fmin(1400.0 - h, top));
                        }
                    }
                    let object = app.object_value(&shape, left, top, w, h, app.color.get(), app.width.get(), app.object_fill_color.get());
                    app.persist_object(&object)?;
                }
                self.invalidate()?;
                return Ok(true);
            }
            if self.eraser_points.with(Option::is_some) {
                self.erase_point(touch)?;
                let path = self.eraser_points.take();
                let mode = app.eraser_mode.get();
                if mode != "page" {
                    app.apply_erase(&EraserMode::from(mode), &ids::LayerId::from(app.selected.get()), ids::PageNumber(app.page.get()), path.as_ref(), geometry::Radius(app.eraser_radius.get() as f64))?;
                }
            } else if self.points.with(Option::is_some) {
                self.point(touch, pressure()?)?;
                let points = self.points.get();
                if app.scribble_to_erase.get() && self.scribble(points.as_ref()) {
                    app.apply_erase(&EraserMode::from("area"), &ids::LayerId::from(app.selected.get()), ids::PageNumber(app.page.get()), points.as_ref(), geometry::Radius(app.eraser_radius.get() as f64))?;
                } else {
                    let current = self.current.get().unwrap_or_else(|| "null".into());
                    app.undo.with_mut(|u| u.push(current));
                    app.redo.with_mut(Vec::clear);
                    self.emit_final(true)?;
                    if app.replica().remote() {
                        app.save();
                    }
                }
                self.set_points(None);
                self.current.set(None);
            }
        }
        self.invalidate()?;
        Ok(true)
    }
    /// `float centroidX(MotionEvent e)`.
    fn centroid_x(&self, e: &Obj) -> AppResult<f32> {
        let count = jcall!(e, "android/view/MotionEvent", "getPointerCount", "()I")?.int();
        let mut value = 0.0f32;
        for i in 0..count {
            value += jcall!(e, "android/view/MotionEvent", "getX", "(I)F", i)?.float();
        }
        Ok(value / count as f32)
    }
    /// `float centroidY(MotionEvent e)`.
    fn centroid_y(&self, e: &Obj) -> AppResult<f32> {
        let count = jcall!(e, "android/view/MotionEvent", "getPointerCount", "()I")?.int();
        let mut value = 0.0f32;
        for i in 0..count {
            value += jcall!(e, "android/view/MotionEvent", "getY", "(I)F", i)?.float();
        }
        Ok(value / count as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (input, normalizeServerAddress, serverAddressForSettings, serverPortForSettings),
    /// produced by the Java originals on OpenJDK 17.
    const NORMALIZE: &[(&str, Result<&str, &str>, &str, &str)] = &[
        ("", Ok(""), "", ""),
        ("  ", Ok(""), "", ""),
        ("host", Ok("http://host:8765"), "host", ""),
        ("host/", Ok("http://host:8765"), "host/", ""),
        ("host///", Ok("http://host:8765"), "host///", ""),
        ("http://host", Ok("http://host:8765"), "host", ""),
        ("https://host", Ok("https://host:8765"), "host", ""),
        ("http://host:80", Ok("http://host:80"), "host", "80"),
        ("http://host:0", Err("invalid server address"), "host", ""),
        ("http://host:65535", Ok("http://host:65535"), "host", "65535"),
        ("http://host:65536", Err("invalid server address"), "host", "65536"),
        ("http://host:99999999999", Err("invalid server address"), "http://host:99999999999", ""),
        ("http://host:", Ok("http://host::8765"), "host", ""),
        ("10.0.0.2", Ok("http://10.0.0.2:8765"), "10.0.0.2", ""),
        ("10.0.0.2:9000", Ok("http://10.0.0.2:9000"), "10.0.0.2:9000", ""),
        ("192.168.50.129:8765/", Ok("http://192.168.50.129:8765"), "192.168.50.129:8765/", ""),
        ("http://user@host", Ok("http://user@host:8765"), "host", ""),
        ("http://user@host:1", Ok("http://user@host:1"), "host", "1"),
        ("http://host/path", Ok("http://host:8765/path"), "host", ""),
        ("http://host/path/?q=1#f", Ok("http://host:8765/path/?q=1#f"), "host", ""),
        ("http://host?q", Ok("http://host:8765?q"), "host", ""),
        ("http://host#frag", Ok("http://host:8765#frag"), "host", ""),
        ("my_host", Err("invalid server address"), "my_host", ""),
        ("my-host.example.com", Ok("http://my-host.example.com:8765"), "my-host.example.com", ""),
        ("bad-.com", Err("invalid server address"), "bad-.com", ""),
        ("1.2.3.400", Err("invalid server address"), "1.2.3.400", ""),
        ("1.2.3.4.5", Err("invalid server address"), "1.2.3.4.5", ""),
        ("[::1]", Ok("http://[::1]:8765"), "[::1]", ""),
        ("[::1]:9000", Ok("http://[::1]:9000"), "[::1]:9000", ""),
        ("http://[fe80::1%25eth0]:80", Ok("http://[fe80::1%25eth0]:80"), "[fe80::1%25eth0]", "80"),
        ("[::1", Err("Expected closing bracket for IPv6 address at index 11: http://[::1"), "[::1", ""),
        ("ho st", Err("Illegal character in authority at index 7: http://ho st"), "ho st", ""),
        ("http://", Ok("http://http::8765"), "http://", ""),
        ("https://", Ok("http://https::8765"), "https://", ""),
        ("ftp://x", Ok("http://ftp::8765//x"), "x", ""),
        ("HTTP://host", Ok("http://HTTP::8765//host"), "host", ""),
        ("host:abc", Err("invalid server address"), "host:abc", ""),
        ("-host", Err("invalid server address"), "-host", ""),
        ("a.1b", Err("invalid server address"), "a.1b", ""),
        ("a.b1", Ok("http://a.b1:8765"), "a.b1", ""),
        ("%41", Err("invalid server address"), "%41", ""),
        ("h%41st", Err("invalid server address"), "h%41st", ""),
        ("x:y@z", Ok("http://x:y@z:8765"), "x:y@z", ""),
        ("été.fr", Err("invalid server address"), "été.fr", ""),
        ("http://host:8765", Ok("http://host:8765"), "host", ""),
        ("http://h:08765", Ok("http://h:08765"), "h", ""),
        ("http://[1:2:3:4:5:6:7:8]", Ok("http://[1:2:3:4:5:6:7:8]:8765"), "[1:2:3:4:5:6:7:8]", ""),
        ("http://[1:2:3:4:5:6:7]", Err("IPv6 address too short at index 8: http://[1:2:3:4:5:6:7]"), "http://[1:2:3:4:5:6:7]", ""),
        ("http://[::ffff:1.2.3.4]", Ok("http://[::ffff:1.2.3.4]:8765"), "[::ffff:1.2.3.4]", ""),
        ("http://host:+80", Err("invalid server address"), "http://host:+80", ""),
        ("http://host:-1", Err("invalid server address"), "http://host:-1", ""),
    ];
    /// (address, port, currentHost, composeServerAddress) from the Java original.
    const COMPOSE: &[(&str, &str, Option<&str>, Result<&str, &str>)] = &[
        ("host", "", None, Ok("http://host:8765")),
        ("host", "", Some("https://x"), Ok("https://host:8765")),
        ("https://host", "", None, Ok("https://host:8765")),
        ("http://host:9000", "", None, Ok("http://host:9000")),
        ("host", "9000", None, Ok("http://host:9000")),
        ("host", "0", None, Err("Port must be between 1 and 65535")),
        ("host", "-5", None, Ok("http://host:8765")),
        ("host", "+80", None, Ok("http://host:80")),
        ("host", "abc", None, Err("Invalid port")),
        ("host", "70000", None, Err("Port must be between 1 and 65535")),
        ("", "", None, Ok("")),
        ("", "80", None, Err("Port requires a server address")),
        ("host/", "", None, Ok("http://host:8765")),
        ("host/x", "", None, Err("Enter only a host or IP address")),
        ("user@host", "", None, Err("Enter only a host or IP address")),
        ("host?q", "", None, Err("Enter only a host or IP address")),
        ("host#f", "", None, Err("Enter only a host or IP address")),
        ("[::1]", "", None, Ok("http://[::1]:8765")),
        ("my_host", "", None, Err("Enter only a host or IP address")),
        ("host:99999", "", None, Err("Port must be between 1 and 65535")),
        (" host ", "  ", None, Ok("http://host:8765")),
        ("1.2.3.4", "8080", Some("https://old:1"), Ok("https://1.2.3.4:8080")),
        ("ho st", "", None, Err("Illegal character in authority at index 7: http://ho st")),
    ];

    #[test]
    fn server_addresses_match_java() {
        for (input, normalized, host, port) in NORMALIZE {
            let actual = MainActivity::normalize_server_address(input);
            match normalized {
                Ok(expected) => assert_eq!(actual.as_deref().ok(), Some(*expected), "normalize {input:?}"),
                Err(_) => assert!(actual.is_err(), "normalize {input:?} gave {actual:?}"),
            }
            assert_eq!(MainActivity::server_address_for_settings(Some(input)), *host, "host {input:?}");
            assert_eq!(MainActivity::server_port_for_settings(Some(input)), *port, "port {input:?}");
        }
        assert_eq!(MainActivity::server_address_for_settings(None), "");
        assert_eq!(MainActivity::server_port_for_settings(None), "");
    }

    #[test]
    fn compose_matches_java() {
        for (address, port, current, expected) in COMPOSE {
            let actual = MainActivity::compose_server_address(address, port, *current);
            match expected {
                Ok(expected) => assert_eq!(actual.as_deref().ok(), Some(*expected), "compose {address:?} {port:?}"),
                Err(message) if message.contains(" at index ") => assert!(actual.is_err(), "compose {address:?}"),
                Err(message) => assert_eq!(actual.err().and_then(|e| e.message()).as_deref(), Some(*message), "compose {address:?} {port:?}"),
            }
        }
    }

    #[test]
    fn java_number_helpers() {
        assert_eq!(parse_java_int("+80"), Some(80));
        assert_eq!(parse_java_int("-5"), Some(-5));
        assert_eq!(parse_java_int("2147483647"), Some(i32::MAX));
        assert_eq!(parse_java_int("-2147483648"), Some(i32::MIN));
        assert_eq!(parse_java_int("2147483648"), None);
        assert_eq!(parse_java_int(""), None);
        assert_eq!(parse_java_int("-"), None);
        assert_eq!(parse_java_int("1 "), None);
        assert!(fmin(f32::NAN, 1.0).is_nan());
        assert!(fmin(1.0, f32::NAN).is_nan());
        assert!(fmax(1.0, f32::NAN).is_nan());
        assert!(fmin(0.0, -0.0).is_sign_negative());
        assert!(fmax(-0.0, 0.0).is_sign_positive());
        assert_eq!(fmax(0.2, fmin(5.0, 7.5)), 5.0);
        assert_eq!(strip_trailing_slashes("a//"), "a");
    }

    #[test]
    fn obj_keeps_java_number_forms() {
        let o = MainActivity::obj(&[("kind", JV::from("object")), ("page", JV::Int(2)), ("x", JV::Float(333.33334)), ("w", JV::Float(500.0)), ("fontSize", JV::Int(24))]);
        assert_eq!(o.to_string(), r#"{"kind":"object","page":2,"x":333.33334,"w":500,"fontSize":24}"#);
        let map = parse_map(&o.to_string()).unwrap();
        assert_eq!(json_object(&map).to_string(), o.to_string());
    }
}
