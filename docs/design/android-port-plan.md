# Android app port: skeleton and fill-in plan

Status: complete (2026-11-02): every module is ported, the emulator tier
passes, and the hardware checks pass on the NoteAir6C (ADR 0029). The Java
sources were removed (commit `0534499` has them). Original plan: `apps/android/src/*.rs` has one
module per Java class in `apps/android/src/com/notemaker/`. Every struct
carries all Java fields. Every method and constructor has its final
signature, and most bodies are still `todo!("<Class.method>")`. The crate builds for the host and for
`arm64-v8a`/`x86_64`, and `apps/android/tools/check_jni.py` passes. The
Java sources stay in place as the reference until each module reaches parity.

Binding conventions are in [`rust-port.md`](rust-port.md), especially
"Android porting patterns". This document adds the conventions specific to the
skeleton, the work-package split, and the cross-package contracts.

```sh
nix-shell --run 'cargo build -p notemaker_android'
nix-shell --run 'cargo ndk -t arm64-v8a -t x86_64 -P 26 build -p notemaker_android'
nix-shell --run 'python3 apps/android/tools/check_jni.py'   # before every device build
```

## Modules

| Java (lines / bytes) | Rust module | Package | State |
| --- | --- | --- | --- |
| `MainActivity` incl. `CanvasView`, `CanvasView$1`, `ErasePreview`, `PendingObjectHistory` (893 / 106,897) | `main_activity` | A | lifecycle routing, `new`, `from_java`, `run_on_ui_thread` done; rest `todo!` |
| `EditorUi` incl. `Tool`, `Icon`, `LayerPreview`, `StrokeSample`, `ConflictPreview`, `$1`–`$6` (1,062 / 80,885) | `editor_ui` | B | trivial helpers (`dp`, `column`, `row`, `divider`, `space`, `text`, `surface`, `selected_tool`) done |
| `Integration` (412 / 53,719) | `integration` | C | routing, `instance`, `on_create`, `Instrumentation` wrappers done |
| `CoverageCheck` (182 / 18,683) | `coverage_check` | C | `todo!` |
| `StrokeCheck` (78 / 5,910) | `stroke_check` | C | `todo!` |
| `ViewportCheck` incl. `Frame` (236 / 24,834) | `viewport_check` | D | `todo!` |
| `InteractionPerformance` (298 / 24,116) | `interaction_performance` | D | `todo!` |
| `Performance` (173 / 16,980) | `performance` | D | constants done |
| `ErasePerformance` (47 / 4,144) | `erase_performance` | D | `todo!` |
| `CheckpointPerformance` (42 / 2,122) | `checkpoint_performance` | D | `todo!` |
| `StrokeCrossings` (51 / 3,888) | `stroke_crossings` | D | `todo!` |
| `AgentUi` incl. `Item`, `Work`, `Result`, `$1` (344 / 21,355) | `agent_ui` | E | `dp`, `column` done |
| `TileInkCache` incl. `Item`, `Key`, `Tile`, `PathEntry`, `BoundRecord` (131 / 17,109) | `tile_ink_cache` | E | statics/constants done |
| `DocumentBrowser` incl. `$1`, `$2` (225 / 14,525) | `document_browser` | E | `todo!` |
| `AnimationPlayer` incl. `Clip`, `Hit`, `$1`, `$2` (126 / 14,225) | `animation_player` | E | `todo!` |
| `UiActions` incl. `Action` (82 / 11,139) | `ui_actions` | E | `UiSession` impl done |
| `InkCache` (69 / 5,456) | `ink_cache` | E | `todo!` |
| `ToolDisplay` (76 / 3,506) | `tool_display` | E | `todo!` |
| `AppReplica` (59 / 3,486) | `app_replica` | — | **complete** |
| `ReplicaService` (30 / 1,891) | `replica_service` | — | **complete** |
| `apps/cli/android/.../Png.java` + `ItemCoverage.java` (Canvas side) | `android_canvas` | — | **complete** (`AndroidCanvas` implements `notemaker_cli::canvas::Canvas`) |

Infrastructure modules: `jx`, `field`, `monitor`, `org_json`, `components`
(verified on the emulator before this work), plus the following modules added for the skeleton:

| Module | Contents |
| --- | --- |
| `error` | `AppError {Java(JavaError), Core(notemaker_core::Error), Json(JSONException)}`, `AppResult<T>`; `From` for `JavaError`, core `Error`, `JSONException`, `std::io::Error` (→ `Core` with kind `Io`), boxed `UiError`; `Display` = Java `Throwable.toString()`; `message()`, `class()`, `is("java.io.IOException")` (catch semantics incl. superclasses), constructors `runtime`, `runtime_cause`, `runtime_with`, `illegal_argument`, `illegal_state`, `io`, `java`; `error::require` (`Model.require`). `AppError → JavaError` for native callbacks, `AppError → core::Error` for core/CLI signatures. |
| `concurrent` | `ExecutorService` (`named`, `single_thread`, `execute`, `submit → Future`, `shutdown_now`), `Future` (`get`, `get_timeout`, `cancel`), `CountDownLatch` (`count_down`, `await_`, `await_timeout`). Workers are `JThread`s (cooperative interrupt). |
| `android_consts` | every Android compile-time constant the Java uses (values from `javap -constants` on API 36 `android.jar`), named `CLASS_FIELD` (`MOTIONEVENT_ACTION_DOWN`, `LAYOUT_PARAMS_MATCH_PARENT`, `DIALOGINTERFACE_BUTTON_POSITIVE`, `R_LAYOUT_SIMPLE_SPINNER_DROPDOWN_ITEM`, …). |
| `android_canvas` | `AndroidCanvas` (`Png::new(&mut AndroidCanvas::new(canvas_obj))`), `path_to_java`, `rect_to_java`/`rect_from_java`, `matrix_values`, `canvas_transform`, `pixmap_to_bitmap`, `bitmap_pixels`, cached enum objects (`style_obj`, `cap_obj`, `join_obj`, `fill_type_obj`, `argb_8888`), `RectFExt` (Android `RectF` methods missing from `canvas::RectF`). |
| `ui` | Java helper mirrors and inline idioms: `round_f32` (`Math.round(float)`), `density`, `dp`, `view`, `column`, `row`, `linear_layout`, `frame_layout`, `text_view`, layout params, `set_margins`, `add_view(_params)`, `set_padding`, `set_text`, `text_of`, `text` (= `EditorUi.text`), `surface` (= `EditorUi.surface`), `on_click`, `toast`, preferences (`prefs`, `pref_*`, `edit`, `put_*`, `apply`), `files_dir`, `cache_dir`, `main_handler`, `on_main_thread`, `post`, `post_delayed`, `remove_callbacks`, `view_post`. |

Additions to `jx` (existing APIs unchanged): `jset!` / `set_field` (field
writes, checked by `check_jni.py`), `once_runnable`, `try_once_runnable`,
`handle`, `mask::{DRAW, DISPATCH_DRAW, TOUCH, DETACHED, SIZE, ATTACHED}`,
`native_view`, `native_linear_layout`, `native_frame_layout`,
`scale_listener`, `adapter`, `WeakObj` (JNI weak global reference).
`check_jni.py` now checks `jset!` and nested-class constructors.

`apps/android/Cargo.toml` now depends on `notemaker_cli` (for `Png`,
`ItemCoverage`, `LocalProxy`, and `canvas`) and on `resvg` 0.45, with the same
version and features as the CLI. The `resvg` dependency only names the
`tiny_skia`/`usvg` types in the `Canvas` trait.

## Skeleton conventions

### Types

| Java | Rust |
| --- | --- |
| Android object (`View`, `Bitmap`, `Canvas`, `Paint`, `Intent`, `Bundle`, `Handler`, …) | `jx::Obj` (`Option<Obj>` when nullable) |
| `org.json.JSONObject/JSONArray` | `org_json::JSONObject/JSONArray` (reference semantics) |
| `Map<String,Object>` (JSON data, results, `state()`, metrics) | `notemaker_core::json::Map` |
| `Map<String,Object>` that is a model item (identity matters: `IdentityHashMap`, `==` shortcuts, `TileInkCache.Item.value`, `renderedEntities`, `pendingObjectPreviews`) | `notemaker_core::model::Entity` (a newtype over `Arc<EntityContent>`); identity = `Entity::ptr_eq` / `Arc::as_ptr(&entity.0)` |
| `List<Object>` | `Vec<Value>`; `Object` input → `Option<Value>` / `&Value` |
| `LinkedHashMap` / `LinkedHashSet` | `IndexMap` / `IndexSet` (use `shift_remove`) |
| `float`, `int`, `long`, `double` | `f32`, `i32`, `i64`, `f64` |
| `RectF` used as data | `notemaker_cli::canvas::RectF` + `android_canvas::RectFExt` |
| `android.graphics.Path` built once and reused (`ErasePreview.paths`) | `Obj` (from `android_canvas::path_to_java`) |
| `Runnable` posted to a `Handler` and later removed | `Obj` (a `jx::runnable`), so `removeCallbacks` sees the same object |
| `Runnable`/`Consumer`/`LongConsumer` fields called from Rust | `Arc<dyn Fn(..) + Send + Sync>` (`main_activity::{Callback, StringConsumer, LongConsumer}`) |
| `BiConsumer<Canvas,Map>` renderer | `&mut dyn FnMut(&Obj, &Entity) -> AppResult<()>` (`tile_ink_cache::Renderer`, `ink_cache::StrokeRenderer`) |
| `List<String> args` for replica commands | `&[&str]` |
| `static` mutable counters | atomics (`tile_ink_cache::TILE_RENDERS`, …) |
| `synchronized(lock)` | `app.lock.enter()` (`Monitor`, reentrant) |
| Java overloads | distinct names: `apply`/`apply_with`, `enqueue`/`enqueue_with`, `emit`/`emit_final`, `insert_svg`/`insert_svg_template`, `command`/`command_named`, `apply_pen_preset`/`apply_pen_preset_values`, `save_pen_preset`/`save_pen_preset_named`, `cache_pixels`/`cache_pixels_sized`, `Item::new`/`Item::with_picture` |

Most methods return `AppResult<T>`. Java unchecked exceptions propagate with
`?` and reach `jx::guard` in the native callback, which rethrows them into
Java, so they still crash where Java crashed. Methods that only compute in Rust
return plain values. `MainActivity::obj` panics where Java threw a
`RuntimeException`.

### Ownership and lifetimes (no cycles through JNI global refs)

A JNI global reference is a GC root, so a Rust → Java → Rust cycle can never
be collected. The skeleton follows these rules:

1. **Owner structs** (`MainActivity`, `EditorUi`, `AgentUi`, `DocumentBrowser`,
   `AnimationPlayer`, `UiActions`, `AppReplica`) are held by `Arc` in
   `MainActivity` fields. Back-references are `Weak<MainActivity>`, and
   `app()` upgrades them. Owner structs may hold strong `Obj`s to views.
2. **Closures stored in Java objects** (listeners from `jx::callback`, native
   view handles, adapters, the `AnimationPlayer.tick` runnable) capture
   owners **weakly** and upgrade on each call. One-shot posted runnables
   (`run_on_ui_thread`, `Handler.post`, `once_runnable`) may capture an `Arc`,
   because the message queue drops them after they run.
3. **View structs** (`CanvasView`, `Tool`, `Icon`, `LayerPreview`,
   `StrokeSample`, `ConflictPreview`, `ChoiceItem`, `ColorSwatch`,
   `PopupShell`) are created by their constructor as `Arc<Self>`. The
   constructor creates the Java view with `jx::native_view` /
   `native_linear_layout` / `native_frame_layout`, and the handle closure
   holds the `Arc` and calls `dispatch`. The struct stores its own Java view
   in `view: OnceLock<WeakObj>` (use `view()`), and any other view of the
   same tree as a `WeakObj` (for example `Tool.label`). A child view
   references its parent, so a strong global ref to a child keeps the whole
   tree alive. Owners may also hold the `Arc` (for example
   `EditorUi.tools`, `MainActivity.canvas`).
4. **Worker threads** capture `Arc<MainActivity>` for the duration of their
   work, as Java's strong references did.

### MainActivity handle and lifecycle

- `components::dispatch` routes `com.notemaker.MainActivity` events to
  `main_activity::component_event`.
- `onCreate`: `MainActivity::new(this)` initializes the Java fields
  (`document="notes"`, `type_="fixed"`, `selection_handler` on the main
  looper, and so on). `Arc::into_raw` is written to the shim's
  `public long handle` with `jset!`. Then `on_create(bundle)` runs, after
  the shim's `super.onCreate`.
- `onStart`, `onStop` and `onActivityResult` call `MainActivity::from_java(component)`,
  which clones the `Arc` from `handle`, and then call the stubbed method.
- `onDestroy`: `on_destroy()` runs the Java logic (before the shim's
  `super.onDestroy()`). Then `handle` is set to 0 and the raw `Arc` is
  dropped. Workers that still hold an `Arc` keep the instance alive until they
  finish.
- Any Rust code that holds a Java activity `Obj` uses
  `MainActivity::from_java(&obj)`. Code that already has the Rust instance
  uses `app.this` as the `Context`.

### Fields and locks

Mutable fields are `Field<T>`. `Field::with_mut` panics on a nested mutable
access to the **same** field, so keep closures leaf-level: copy out with
`get()`, compute, then `set()`. Never call back into app code while inside
`with_mut`. `Monitor` (`app.lock`) is reentrant, as `synchronized` was.
Acquire `app.lock` before any `Field` lock, never while holding one.
`model: Field<Arc<Model>>`: `refreshShared` replaces it, `apply` mutates it
with `Arc::make_mut`, and `Model before = model` is `app.model()`.

## Work packages

Each package edits only the bodies in its own files. A package that needs a
signature change, or a helper in a file it does not own, asks the owner (or
the coordinator) first. Shared infrastructure (`jx`, `ui`, `error`,
`concurrent`, `android_canvas`, `android_consts`, `field`, `monitor`,
`org_json`) may be **extended** by any package with new functions only.
Coordinate when two packages add the same helper, and never change an
existing signature. `lib.rs`, `components.rs`, `app_replica.rs` and
`replica_service.rs` are finished. Change them only for a bug fix, after
review.

| Package | Owns | Java bytes |
| --- | --- | --- |
| **A** | `main_activity.rs` (`MainActivity`, `CanvasView`, `ErasePreview`) | 106,897 |
| **B** | `editor_ui.rs` | 80,885 |
| **C** | `integration.rs`, `coverage_check.rs`, `stroke_check.rs` | 78,312 |
| **D** | `performance.rs`, `interaction_performance.rs`, `erase_performance.rs`, `checkpoint_performance.rs`, `viewport_check.rs`, `stroke_crossings.rs` | 76,084 |
| **E** | `agent_ui.rs`, `document_browser.rs`, `animation_player.rs`, `ui_actions.rs`, `tile_ink_cache.rs`, `ink_cache.rs`, `tool_display.rs` | 87,315 |

A, B and E produce a runnable app. C and D can only be exercised on a device
or emulator once A, B and E work. They can be written in parallel and checked
for compile errors and JNI literals against the skeleton.

### Cross-package contracts (callee → callers)

- **A (`MainActivity`)** is used by every package. Callers read and write
  its `pub` fields directly. They hold `app.lock` where the Java code did.
  The methods called from elsewhere are:
  `update`, `update_chrome`, `save`, `deselect`, `reset_drawing_gesture`,
  `open_document`, `history`, `select_layer`, `select_all_visible`,
  `start_selection`, `set_drawing_tool`, `insert_template`,
  `choose_gif/choose_picture/choose_svg_picture`, `document_dialog`,
  `page_dialog`, `object_dialog`, `text_placement_dialog`, `rename_layer`,
  `layer_command`, `fill_entity`, `resolve_conflict`, selection commands,
  `save_network_settings`, `persist_object`, `enqueue(_with)`,
  `apply_erase`, statics `obj`, `id`. The server address statics
  (`compose_server_address`, `*_for_settings`, `normalize_server_address`)
  are now `network::ServerAddr::{compose, settings_host, settings_port,
  normalize}` (ADR 0030).
  `CanvasView` methods used elsewhere are `scale`, `visible`,
  `invalidate(_rect)`, `stroke`, `draw_stroke`, `tile_cache_metrics`,
  `document_viewport`, `on_touch_event`, `erase_at`, `emit`, `view()`
  (`getWidth`/`getHeight`/`draw(Canvas)`/`dispatchTouchEvent` through
  JNI on it), and `ErasePreview::new`.
  A constructs `EditorUi::new` + `build`, `AgentUi::new`, `AnimationPlayer::new`,
  `UiActions::new`, `AppReplica::new`, and `TileInkCache::new` (in
  `CanvasView::new`).
- **B (`EditorUi`)** provides `new`, `build` (which must create
  `CanvasView::new` and set `app.canvas`), `dp`, `refresh`, `selected_tool`,
  `dismiss_popup`, `open_panel`, `open_tool`, `show_files`
  (`DocumentBrowser::new(app)?.show()`), `show_tool_labels`,
  `panel_showing`, `save_pen_preset_named`, `apply_pen_preset`,
  `show_conflict_panel`, `show_selection_panel`.
  B calls into `InkCache` (E) from `LayerPreview` and into
  `AgentUi::show` / `AnimationPlayer::show` (E).
- **C (`Integration`)** provides `run_on_main_sync`, `wait_for_idle_sync`
  (done), the statics `gesture`, `live_gesture` and `touch`, and `drain_writes`.
  D's `ErasePerformance` calls `Integration::gesture` and
  `coverage_check::cache_pixels`. C's `run_action` calls every D entry
  point (`performance::run`, `interaction_performance::run`,
  `erase_performance::run`, `checkpoint_performance::run`,
  `viewport_check::run`) and `DocumentBrowser::thumbnail` (E).
  `stroke_check::run` calls `stroke_crossings::run` (D).
- **D** calls `performance::stats/resources` internally, plus A's
  `CanvasView::tile_cache_metrics` and `TileInkCache::bounds_cache_metrics` and
  `tile_size` (E).
- **E**: `TileInkCache` (`new`, `begin_frame`, `clear`, `draw`,
  `draw_monolithic`, `density_tier`, `tile_size`, `metrics`, `cache_clears`,
  `resident_page_count`, the counter statics, `Item::new/with_picture`) is
  used by A. `AnimationPlayer` (`loaded`, `prepare_visible`, `draw`,
  `draw_controls`, `touch`, `pause_all`, `stop`, `reset`, `show`, `select`,
  `play`, `step`, `seek`, `state`) is used by A, B and UiActions.
  `AgentUi` (`show`, `dismiss`, `showing`, `destroy`) is used by A, B and
  UiActions. `DocumentBrowser` (`new`, `show`, `dismiss`, `showing`,
  `search`, `mode`, `refresh_now`, `state`, `thumbnail`) is used by B, UiActions
  and C. `ToolDisplay::new` is used by `AgentUi::Item`.

### Threading notes

- **UI thread**: all lifecycle methods, all `EditorUi`, `AnimationPlayer`,
  `DocumentBrowser` and `AgentUi` UI code, `CanvasView` callbacks, and every
  `run_on_ui_thread` body. `UiActions` actions run on the UI thread: `request_impl`
  posts to `main` and waits up to 30 s, or runs inline when it is already on
  the main looper (`ui::on_main_thread`).
- **`replica-writer`** (`replica_service::writes()`): every replica command
  the editor issues (`persist`, `persist_object`, `history`, selection,
  transform, erase, fill, template, layer, conflict, import decoding) and the
  DocumentBrowser listing/delete/create. Results return through
  `run_on_ui_thread`. Tests drain it with `writes().submit(|| ()).get_timeout(30 s)`.
- **`local-state`** (`MainActivity.local_thread`): `local_loop`. Remote:
  `AppReplica::listen` (blocking SSE). Local: `LocalProxy::await_change_timeout`
  in a loop that checks `JThread::current_interrupted()` and `running`, since
  Rust cannot interrupt `await_change`. Stopped with `JThread::interrupt` in
  `configure_proxy`, and with `AppReplica::close` (disconnect) for remote streams.
- **AgentUi**: `requests` executor for HTTP work; a reader thread per
  `subscribe`; `ui` handler posts back. `synchronized(incoming)` →
  `incoming_lock`.
- **DocumentBrowser**: `previews` executor renders thumbnails
  (`DocumentBrowser::thumbnail` is thread-agnostic and only creates Java
  `Bitmap`/`Canvas`). Loops over many JNI calls on Rust threads need
  `jx::frame`.
- **Instrumentation thread** (C/D): `on_start`, `serve`, `run_action` and all
  checks/benchmarks. UI access goes through `test.run_on_main_sync`. Benchmarks
  create `HandlerThread`s (`performance-frames`, `interaction-frames`) for
  `FrameMetrics` listeners via JNI and `jx::callback`.
- **`synchronized(app.lock)` regions** in Java:
  - `MainActivity` (27): `setDrawingTool`, `persistObject` (enqueue part),
    `objectHistoryPending`, `load`, `update`, `updateChrome`, the dialog
    callbacks in `documentDialog`, `saveNetworkSettings` and `openDocument`,
    `history` (start and UI result), `objectDialog`, the pending-history
    sections of `insertTemplate`/`fillEntity`/`layerCommand`/
    `commitTransformPreview`/`selectionCommand`/`applyErase`/`fillAt`,
    `renameLayer`'s save callback, `selectRegion`, `editObject` and its
    dialog callbacks, `CanvasView.onDraw`, and `CanvasView.onTouchEvent`.
  - `EditorUi` (9): `rebuildLayers` (layer list, card click and visibility
    callbacks), `newLayer`, `layerOptions` (two callbacks), `changePage`,
    `chooseEntities`, and `LayerPreview.onDraw`'s object loop.
  - `Integration` (3): `liveGesture` steps and two `runAction` blocks (one inside `runOnMainSync`).
  - Other monitors: `AgentUi` `synchronized(incoming)` (3);
    `ReplicaService.class` (done); the `TileInkCache` static
    synchronized bound cache (`BOUNDS` mutex).

## Open points and decisions for fill-in

- **SVG pictures**: Java's Android `Png` replayed cached `Picture`s for SVG
  objects. `AndroidCanvas::draw_svg` rasterizes with resvg on the device pixel
  grid on every draw and uploads a temporary `Bitmap`. Tiles cache the
  result, but uncached paths (previews, `LayerPreview`, thumbnails) pay the
  full cost. Measure on hardware. If needed, add a pixmap→`Bitmap` cache in
  `android_canvas`.
- **Paths per draw**: `AndroidCanvas::draw_path` rebuilds a Java `Path` for
  every call. `TileInkCache.play` replays cached `StrokeCommands`. If this is
  slow, convert each `StrokeCommands` once to Java `Path`s (option 2 in
  `apps/cli/RUST.md`).
- **`java.net.URI`**: `normalize_server_address`, `compose_server_address`
  and the `*_for_settings` functions need a hand port of the URI parsing they
  rely on (host, port, raw authority/path/query/fragment, user info). No URL
  crate is on the allowed list.
- **HTTPS**: the core `Endpoint`/`HttpConnection` is HTTP only, so
  `https://` proxy URLs (remote replica, `AgentUi.http`) fail with an `Io`
  error until a TLS decision is made (rust-port.md).
- **`startCli`** returns `std::process::Child` and must exec
  `nativeLibraryDir/libnotemaker_cli.so` (rust-port.md "Android"), not
  `app_process`. Integration's CLI checks depend on it.
- **Instrumentation signature**: `Performance.run(Instrumentation, …)` and
  `InteractionPerformance.run(Instrumentation, …)` take `&Arc<Integration>`,
  the only instrumentation in the process.
- **Clippy**: the existing `jx::call`/`call_nonvirtual` (and the new
  `set_field`) trip `clippy::not_unsafe_ptr_arg_deref`. `cargo clippy` was not
  clean before this work either. This is not a build requirement.
- Rust cannot interrupt `LocalProxy::await_change`, so `local_loop` must use
  `await_change_timeout` (documented in apps/cli/RUST.md).
