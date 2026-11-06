# notemaker_cli (Rust port of the Java CLI)

Crate `apps/cli` → library `notemaker_cli` plus binary `notemaker`. It ports
`apps/cli/{src,jvm,android}/com/notemaker/cli/*.java` one-to-one and the
`tests/*.java` probes as extra binaries. Conventions are binding and live in
[`docs/design/rust-port.md`](../../docs/design/rust-port.md). The core API is
[`packages/core/README.md`](../../packages/core/README.md).

```sh
nix-shell --run 'cargo build -p notemaker_cli'
nix-shell --run 'cargo ndk -t arm64-v8a -P 26 build -p notemaker_cli'
```

## Modules

| Java | Rust |
| --- | --- |
| `Main` | `notemaker_cli::main` (file `src/cli_main.rs`; `src/main.rs` is not a bin, `autobins = false`) |
| `LocalProxy`, `LocalProxy.UiSession` | `local_proxy::{LocalProxy, UiSession, UiError}` |
| `UiHub` (package-private) | `ui_hub::UiHub` |
| `Png` (jvm + android), `Png.StrokeCommands` | `png::{Png, StrokeCommands, StrokeCommand, USE_SVG_PICTURES, USE_DIRECT_DOTS, TRACE_SVG}` |
| `ItemCoverage` (jvm + android) | `item_coverage::{ItemCoverage, create, path, FACTORY, Factory, cache_len, cache_clear}` |
| `android.graphics.{Canvas,Paint,Path,RectF}` / AWT `Graphics2D`, `Path2D` | `canvas::{Canvas, Paint, Path, RectF, FillType, Verb, Style, Cap, Join}` (backend-neutral) |
| `Bitmap`/`BufferedImage` + `Canvas`/`Graphics2D` | `raster::RasterCanvas` (tiny-skia `Canvas` backend) |
| AWT `Font` / `Typeface`, `Png.ensureFonts` | `typeface::{Typeface, ensure_fonts, font_dirs, svg_fontdb}` |
| `tests/StrokeSvgRaster.java` | bin `stroke_svg_raster IN.svg OUT.png` |
| `tests/CoverageParity.java` | bin `coverage_parity` |
| `tests/LatencyProbe.java` | bin `latency_probe DIR DOCUMENT` |
| `tests/CacheFootprintProbe.java` | bin `cache_footprint_probe` |
| `LatencyProbe.memory/timed`, `java.util.Random` | `probe::{memory, timed, JavaRandom}` |

Errors are `notemaker_core::Error` (Java class + exact message; `Display` is
`Throwable.toString()`). Java `null` results are `Option`/`Value::Null` as noted.

## `Main`

| Java | Rust |
| --- | --- |
| `public static final String HELP` | `pub const HELP: &str` (byte-identical) |
| `public static void main(String[])` | `pub fn main(args: Vec<String>) -> i32` (exit code; args exclude argv[0]) |
| `public static Object run(String[]) throws Exception` | `pub fn run(args: &[String]) -> Result<Option<Value>>` (`None` = `null`) |
| `static Map snapshot(Map state)` | `pub fn snapshot(s: &State) -> Result<Map>` |
| (`Store.Snapshot` returned by `snapshot`) | `pub fn snapshot_state(s: &State) -> Result<State>`: the same map, carrying the model (ADR 0027) |
| `static ThreadLocal<Object> proxyInput` | `thread_local! PROXY_INPUT: RefCell<Option<Value>>` |
| `static ThreadLocal<Boolean> inProxy` | `thread_local! IN_PROXY: Cell<bool>` |

`main` prints the result's JSON to stdout. On failure it prints
`{"error": rootCause.toString()}` to stderr and returns 1.

## `LocalProxy`

```rust
pub type UiError = Box<dyn std::error::Error + Send + Sync>;
pub trait UiSession: Send + Sync {
    fn request(&self, verb: &str, action: &str, input: Map) -> Result<Value, UiError>;
}
pub struct LocalProxy { pub connected: Mutex<HashMap<String,bool>>, pub errors: Mutex<HashMap<String,String>>, /* private */ }
```

| Java | Rust |
| --- | --- |
| `interface UiSession { Object request(String verb,String action,Map input) throws Exception; }` | `trait UiSession: Send + Sync { fn request(&self, verb: &str, action: &str, input: Map) -> Result<Value, UiError>; }`. The error's `Display` is used where Java called `error.toString()`. |
| `new LocalProxy(File directory, int port) throws IOException` | `LocalProxy::new(directory: &ReplicaDir, port: Port) -> Result<Arc<LocalProxy>>` |
| `void configure(String document, String target) throws Exception` (synchronized) | `fn configure(&self, document: &str, target: Option<&str>) -> Result<()>` |
| `Object execute(Map body) throws Exception` | `fn execute(&self, body: &Map) -> Result<Value>` |
| `static Object request(Map descriptor, String document, String actor, List<String> args, Object input)` | `LocalProxy::request(descriptor: &Map, document: &str, actor: &str, args: &[String], input: Option<&Value>) -> Result<Value>` |
| in-process `execute` of `snapshot` (Java returned the `Store.Snapshot` object) | `LocalProxy::snapshot_state(document: &str) -> Result<State>`, reusing the last reply while the journal map, previews and leases are unchanged |
| `void close() throws IOException` | `fn close(&self) -> Result<()>` (idempotent; joins all threads, then releases `proxy.lock`) |
| `void registerUiSession(String, UiSession)` | `fn register_ui_session(&self, session: &str, handler: Arc<dyn UiSession>) -> Result<()>` |
| `void unregisterUiSession(String)` | `fn unregister_ui_session(&self, session: &str)` |
| `void setUiUpstream(Map descriptor)` (nullable) | `fn set_ui_upstream(&self, descriptor: Option<&Map>)` |
| `long version()` | `fn version(&self) -> i64` |
| `long awaitChange(long) throws InterruptedException` | `fn await_change(&self, previous: i64) -> i64`; plus `await_change_timeout(previous, ChangeWaitTimeout) -> i64` (see below) |
| `int port()` | `fn port(&self) -> i32` |
| `void await()` | `fn await_(&self)` |
| `Object preview(String document,String actor,String session,List entities,Map transform)` | `fn preview(&self, document: &str, actor: &str, session: &str, entities: &[Value], transform: &Map) -> Result<Map>` |
| `static Object select(Store, String actor, Map request)` | `LocalProxy::select(store: &Store, actor: &str, request: &Map) -> Result<Map>` |
| `static void assertSession(Store, String actor, String session, List entities)` | `LocalProxy::assert_session(store, actor, session, entities: &[Value]) -> Result<()>` |
| `static List selected(Store, String actor, String session)` | `LocalProxy::selected(store, actor, session) -> Result<Vec<Value>>` |
| `static Object release(Store, String actor, String session, boolean heartbeat)` | `LocalProxy::release(store, actor, session, heartbeat: bool) -> Result<Map>` |
| `static Object releaseActor(Store, String actor)` | `LocalProxy::release_actor(store, actor) -> Result<Map>` |
| `public final Map<String,Boolean> connected` / `Map<String,String> errors` | `pub connected: Mutex<HashMap<String,bool>>` / `pub errors: Mutex<HashMap<String,String>>` |

Notes:

- **Lifetime.** The worker threads hold `Arc` clones, so dropping the last
  external `Arc` does not stop the proxy; call `close()`, as in Java.
  `ReplicaService.proxy` becomes `Mutex<Option<Arc<LocalProxy>>>`.
- **Threads.** Threads keep their Java names: `local-proxy`, `proxy-sync`,
  `proxy-notifications` and `proxy-ui-sessions`. Each accepted connection
  and each UI command runs on its own short-lived thread, which replaces the
  cached thread pool. The Java JVM shutdown hook (`notemaker-proxy-shutdown`)
  becomes a SIGINT/SIGTERM handler. It is installed only by the `notemaker`
  binary's `serve` path, not by library users such as the Android app.
- **Constructor errors.** Java wrapped setup failures in
  `new IOException(cause)`, and the CLI printed the root cause. Rust returns
  the root cause itself, so CLI output is unchanged. Only Android log text
  loses the `java.io.IOException: ` prefix.
- **Interrupts.** Rust cannot interrupt a thread blocked in `awaitChange`.
  `await_change` returns when the generation changes or the proxy closes.
  Android's cooperative-interrupt loops use `await_change_timeout` and check
  their interrupt flag between calls.
- **Locks.** `proxy.lock` uses a POSIX `fcntl` `F_SETLK` write lock, which
  matches `FileChannel.tryLock` on Linux and Android. A second `LocalProxy` in
  the same process fails the same way, through an in-process check that
  replaces `OverlappingFileLockException`.

## `UiHub`

| Java | Rust |
| --- | --- |
| `LEASE_MS`, `RESULT_MS` | `UiHub::LEASE_MS`, `UiHub::RESULT_MS` |
| `synchronized Map list()` | `fn list(&self) -> Map` |
| `synchronized Map heartbeat(Map body)` | `fn heartbeat(&self, body: &Map) -> Result<Map>` |
| `synchronized Map command(Map body) throws InterruptedException` | `fn command(&self, body: &Map) -> Result<Map>` (`Mutex` + `Condvar` for `wait`/`notifyAll`) |

## `Png`, `StrokeCommands` and the `Canvas` abstraction

The Android `Png` is a `Render.Surface` over an `android.graphics.Canvas`
and contains most of the rendering logic: erase clips, the ink layering,
image and SVG caches, and dot grids. The Rust `Png` keeps that logic and draws
through `&mut dyn canvas::Canvas`. That trait mirrors the `Canvas` calls the
Java used:
`save`, `restore`, `restore_to_count`, `translate`, `rotate`, `scale`,
`matrix`, `clip_rect`, `clip_out_path`, `draw_color`, `draw_path`,
`draw_line`, `draw_rect`, `draw_oval`, `draw_bitmap`, and `draw_svg` (which
has a default implementation).

- **CLI.** `raster::RasterCanvas` is the tiny-skia backend (pixmap, matrix
  and clip-mask save stack). `Png::export` and `Png::coverage_tile` use it.
- **Android.** The app implements `Canvas` for a struct that wraps its JNI
  `android.graphics.Canvas` (and a cached Java `Paint`). Wherever Java wrote
  `new com.notemaker.cli.Png(canvas)`, the app writes
  `Png::new(&mut jni_canvas)`. Text reaches the backend as glyph outlines
  (`draw_path`), images as decoded `Pixmap`s (`draw_bitmap`), and SVG through
  `draw_svg`. The default `draw_svg` rasterizes with resvg at the current
  device scale and calls `draw_bitmap`. As a result, the Android backend needs
  only path, line, rect, oval, bitmap, matrix and clip calls over JNI.

| Java | Rust |
| --- | --- |
| `public Png(Canvas canvas)` (Android) / `private Png(Graphics2D)` (JVM) | `Png::new(canvas: &mut dyn Canvas) -> Png<'_>` |
| `static int[] coverageTile(Model, Map item, int x, int y, int size)` | `Png::coverage_tile(m: &Model, item: &Map, x: i32, y: i32, size: i32) -> Result<Vec<i32>>` (unpremultiplied ARGB) |
| `static void export(Model, int page, double x, double y, int w, int h, File out)` | `Png::export(m: &Model, page: PageNumber, origin: DocPoint, size: PixelSize, out: &RenderOutputPath) -> Result<()>` |
| `static StrokeCommands compileStroke(Map stroke)` | `Png::compile_stroke(stroke: &Map) -> Result<StrokeCommands>` |
| `static void replayStroke(Canvas, StrokeCommands, Paint)` | `Png::replay_stroke(canvas: &mut dyn Canvas, commands: &StrokeCommands, paint: &mut Paint)` |
| `static void ensureFonts()` | `Png::ensure_fonts() -> Result<()>` (= `typeface::ensure_fonts`) |
| `static boolean useSvgPictures, useDirectDots, traceSvg` | `static USE_SVG_PICTURES, USE_DIRECT_DOTS, TRACE_SVG: AtomicBool` |
| `final class StrokeCommands` (opaque) | `#[derive(Clone)] struct StrokeCommands` (`Arc<[StrokeCommand]>`; `commands() -> &[StrokeCommand]`) |
| `Render.Surface` methods | `impl notemaker_core::render::Surface for Png<'_>` |

`StrokeCommand` makes the recorded lambdas explicit:

```rust
pub enum StrokeCommand {
    SaveClipOut(Vec<Path>),                         // save(); clipOutPath(each)
    Restore,                                        // restore()
    FillPath { path: Path, color: i32 },            // FILL drawPath
    StrokePath { path: Path, color: i32, width: f32 }, // STROKE, join ROUND, drawPath
    Line { x0: f32, y0: f32, x1: f32, y1: f32, color: i32, width: f32 }, // STROKE drawLine
}
```

Stroke command generation (`compile_stroke`) depends only on
`Render::draw_stroke` and the `Png` recorder. It does not use any surface, so
a non-tiny-skia target can use it in either of two ways:

1. Implement `canvas::Canvas` (as above) and call `Png::replay_stroke`.
   This is the one-to-one path for `TileInkCache.play`.
2. Iterate over `commands.commands()` and issue the matching calls directly.
   For example, a JNI backend can turn each `canvas::Path` into a cached
   `android.graphics.Path` once per `StrokeCommands`, using
   `Path::verbs()` / `fill_type()`.

`draw_stroke` only calls `begin_erase`/`end_erase`, `ink`/`polygon` and
`line`, so the recorder covers every stroke. Translucent multi-pass ink is
computed as layered geometry in Rust, the same as `Path.op` on Android. The
recorder therefore emits only `FillPath`s and never needs a canvas-side
boolean operation.

## `ItemCoverage`

| Java | Rust |
| --- | --- |
| `static synchronized Coverage create(Map item)` | `item_coverage::create(item: &Map) -> Result<Arc<dyn Coverage>>` (shared cache entry) |
| method reference `ItemCoverage::create` as `Coverage.Factory` | `&item_coverage::FACTORY` (a static, so `DrawItem`'s per-factory cache sees one identity) |
| `static Path path(double[][] p, boolean close)` | `item_coverage::path(p: &[[f64; 2]], close: bool) -> canvas::Path` (even-odd) |
| private `CACHE` (read by probes via reflection) | `item_coverage::cache_len()`, `cache_clear()` |
| `implements Coverage, Render.Surface` | `impl Coverage for ItemCoverage`, `impl Surface for ItemCoverage` |

## Probes

The probe binaries take the same arguments and print the same output keys:

- `coverage_parity` uses `probe::JavaRandom`, a port of `java.util.Random`,
  so its query sequence matches Java's.
- `latency_probe` / `cache_footprint_probe` report `heap_bytes` from the
  allocator's in-use bytes (`mallinfo`), because there is no GC heap. On
  Android they also report `native_bytes`.

## API changes since the phase-1 skeleton

The phase-1 signatures are unchanged. Additions only:

- `canvas::Path::from_skia(&tiny_skia::Path, FillType) -> Path`, which
  converts stroker output.
- `probe::ms_since(ProbeStart) -> ElapsedMillis` and `probe::print(Value)` for the probe
  binaries.

Core change:

- `notemaker_core::coverage::Painter::paint` now returns
  `Result<Vec<i32>>`. Tile paint errors propagate out of
  `Tiles::intersects`, as in Java.

## Implementation notes and deviations

- **Ordering.** Iteration over Java `ConcurrentHashMap`s (documents, settings,
  previews, UI sessions/results) follows Java's hash order
  (`json::java_hash_set_order`). `proxy-config.json` and listings therefore
  come out in the same key order.
- **Coverage.** `ItemCoverage` follows the JVM `Area` semantics:
  - bounds prefilter: non-empty, strictly overlapping rectangles;
  - hit: positive-area overlap.
  Curves are flattened at 0.01 units.
- **Text.** Glyph advances are fractional (Android `Paint`). The JVM's
  integer-advance `FontRenderContext` and its system font choice are not
  reproduced.
- **Images and SVG.** Images are drawn with nearest-neighbour sampling, as AWT
  `drawImage` did. SVG trees are cached per (source, w, h), LRU 16, because the
  normalized `100%` viewport resolves against the object box.
- **Error classes.** A few Java exception classes have no
  `notemaker_core::ErrorKind`, so the closest kind is displayed:

  | Java exception | Rust display |
  | --- | --- |
  | `java.nio.file.NoSuchFileException` | `java.io.IOException` |
  | `java.net.ConnectException`, `java.net.BindException` | `java.io.IOException` |
  | `TimeoutException` (sync wait) | `IllegalStateException` |

  An `ExecutionException` wrapper is dropped, and the cause is reported
  directly. Messages are unchanged.
- **`UiSession` errors.** Errors that are not core `Error`s are reported as
  `java.lang.IllegalStateException: <Display>`. Android handlers should return
  a core `Error` (boxed) to keep their exact Java class text.
- **Preview refresh race (deliberate deviation).** `sync_once` replaces a
  document's previews with the upstream's `/api/locks` list, as Java did. It
  now skips that replacement when a local preview mutation happened while the
  fetch was in flight, and schedules another fetch. Local mutations are set,
  clear, `clearPreviews` and close, tracked by a private `preview_epoch`.
  Java could resurrect a just-cleared preview from a stale response until the
  next sync. The faster Rust CLI hit that window in
  `selection_checks.test_cross_proxy_preview_clears_on_deselect_and_commit`
  (2 of 12 runs under load; Java 0 of 12).
- **Known, not fixed (core `Store::sync`, same as Java).** A sync that
  snapshots local leases before a concurrent `selection deselect` can POST
  the stale lease upstream, or overwrite local leases with an earlier
  `/api/locks` response. Either resurrects the deselected lease until it
  expires (15 s). Under parallel load this fails the same selection test
  about 1 time in 24.
