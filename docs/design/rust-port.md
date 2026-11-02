# Java → Rust port conventions

Status: complete (2026-11-02; ADR 0029). Core, CLI and Android app are Rust;
the Java sources were removed (last Java commit `0534499`). History: `packages/core` is ported and passes
the parity harness (`packages/core/tests/parity.sh`; see
`packages/core/README.md` for the module table and known differences).
`apps/cli` is ported (crate `notemaker_cli`, API table in `apps/cli/RUST.md`);
`apps/cli/build.sh` and `apps/cli/notemaker` now build and run the Rust binary
and the local test tier passes against it. The
Rust `Endpoint` speaks HTTP and HTTPS (rustls, ADR 0028). Goal: a one-to-one port of every Java
source in `packages/core`, `apps/cli`, `apps/android` and `tests/*.java` to Rust,
keeping behavior, wire formats, persisted files, CLI output and test entry
points identical. Python server and Python tests stay Python.

## Layout

| Java | Rust crate | Notes |
| --- | --- | --- |
| `packages/core/src/com/notemaker/core/*.java` | `packages/core` → crate `notemaker_core` | one module per class, snake_case (`DrawingActions` → `drawing_actions.rs`) |
| `apps/cli/{src,jvm,android}/com/notemaker/cli/*.java` | `apps/cli` → crate `notemaker_cli` (lib) + bin `notemaker` | `jvm/` and `android/` variants collapse into one tiny-skia/resvg implementation |
| `apps/android/src/com/notemaker/*.java` | `apps/android` → cdylib `libnotemaker.so` | thin Java shim in `apps/android/shim/` only forwards callbacks |
| `tests/*.java` probes | `apps/cli` extra bins (`src/bin/*.rs`) | same arguments and output |

The workspace root `Cargo.toml` lists all three crates. Toolchain is pinned in
`rust-toolchain.toml` and provided through `shell.nix` (rustup, cargo-ndk, NDK).
Java sources were removed once every Rust replacement passed its checks;
`packages/core/tests/parity.sh` extracts them from commit `0534499`.

## Values and JSON

- The Java `Map<String,Object>`/`List<Object>` JSON model becomes
  `serde_json::Value` with the `preserve_order` feature (insertion order, like
  `LinkedHashMap`). `notemaker_core::json` re-exports `Value`, `Map`.
- **Never call `Map::remove`** (it is `swap_remove` and reorders keys). Use
  `shift_remove`. Same for `IndexMap`/`IndexSet` ports of `LinkedHashMap`/
  `LinkedHashSet`.
- Java `Long`/`Integer` values → `Value::Number` from `i64`; Java `Double` →
  from `f64` (keep the distinction: `is_i64()` vs `is_f64()`). Java `float`
  values put into JSON print with `Float.toString`; port them as
  `json::from_float(f32)` which stores the f64 whose shortest repr equals the
  float's shortest repr.
- `json::write` and `json::parse` are hand ports of `Json.java` (strict parser,
  depth 64, duplicate-key rejection, Long vs Double by lexical form, exact
  escaping rules). Numbers are formatted with `jfmt::double` / `jfmt::long`.
- `jfmt::double(f64)` reproduces Java `Double.toString` / `StringBuilder.append(double)`
  formatting (shortest round-trip digits; `1.0`, `0.001`, `1.0E-4`, `1.0E7`,
  `-0.0`, `NaN`, `Infinity`). Use it everywhere Java concatenated a double into
  a string (SVG output, messages, file names). `jfmt::float(f32)` likewise.

## Errors

- `notemaker_core::error::{Error, Result}`. `Error` carries a `kind`
  mirroring the Java exception class that was thrown (`IllegalArgument`,
  `IllegalState`, `Io`, `Conflict`, …, add as needed) and the **exact Java
  message**. Code that caught a specific Java exception type matches on `kind`.
  `Model.require(cond,msg)` → `model::require(cond, msg)?`.
- Unchecked exceptions that escaped to the top level in Java become `Err`
  propagated with `?`; do not panic except where Java would crash.

## Concurrency and statics

- `synchronized(x)` → a `Mutex` owned by the same object; `volatile` → atomics
  or a `Mutex`; `ThreadLocal` → `thread_local!`; static mutable singletons →
  `OnceLock`/`Mutex` statics. Thread names are kept (`std::thread::Builder::name`).
- `ExecutorService.newSingleThreadExecutor` → one named worker thread with a
  channel.

## Files, locks, network

- Persisted formats are byte-for-byte compatible: Rust must open state
  directories written by the Java build (the physical tablet's existing data is
  upgraded in place) and write identical bytes for the same operations.
- `FileChannel.lock/tryLock` → POSIX `fcntl` record locks (`F_SETLK(W)`) on the
  same lock files, matching Java on Linux/Android. Within one process, keep the
  Java in-process rules (`OverlappingFileLockException` → same-process check).
- `ServerSocket`/`Socket`/`HttpURLConnection` → `std::net` with the same
  hand-written HTTP/1.1 handling. No async runtime. `HttpsURLConnection` →
  the same client over a blocking rustls stream (`notemaker_core::tls`).
- `UUID.randomUUID()` → `uuid` v4; `UUID.nameUUIDFromBytes` → MD5 name-based
  (v3 bits, no namespace), implemented in `core::ids`.

## Rendering

- `Render.Surface` → `trait Surface`. The CLI implements it with tiny-skia
  (PNG export) and the Android app with `android.graphics.Canvas` over JNI.
- SVG objects render with resvg/usvg (replacing jsvg and androidsvg) on every
  platform. Text uses fonts found through fontconfig paths on the host
  (DejaVu from `shell.nix`) and `/system/fonts` on Android.
- Pixel expectations in tests may need tolerances where resvg differs from the
  old libraries; record such changes in `docs/verification.md`.
- `SvgAsset.viewport` writes `width="100%" height="100%"`. androidsvg and jsvg
  resolve these against the target box and honor `preserveAspectRatio`, but
  usvg resolves them against the viewBox. `Png::svg_tree` therefore writes the
  target size into the root tag before parsing (`fit_root_size`).
- `AndroidCanvas::draw_svg` rasterizes on the device pixel grid (axis-aligned
  matrices, clipped to the canvas) and copies the result 1:1 without
  filtering. A raster drawn at a fractional offset would blur sub-pixel
  shapes, which androidsvg drew as vectors. Other matrices fall back to
  `canvas::draw_svg_raster`.
- Replica state is copy-on-write (`State.map: Arc<Map>`). Do not deep-clone
  journal state on hot paths. See ADR 0027.

## Android

- `apps/android/shim/com/notemaker/*.java` keeps the manifest class names
  (`MainActivity`, `ReplicaService`, `Integration`) plus generic forwarding
  classes (`NativeView`, `NativeLayout`, `NativeCallback`). The shim holds no
  application logic: every override calls a `native` method with a Rust handle.
- Android framework calls (widgets, `Canvas`, `Bitmap`, `Handler`, dialogs)
  go through JNI from Rust, mirroring the original Java calls one-to-one.
- The CLI on Android is the same Rust `notemaker` binary, packaged as
  `lib/<abi>/libnotemaker_cli.so` in the APK and run from the app's
  `nativeLibraryDir` (replacing `app_process … com.notemaker.cli.Main`).

## Allowed crates

`serde_json` (preserve_order), `indexmap`, `uuid`, `md-5`, `sha2`, `base64`,
`crc32fast`, `flate2`, `libc`, `roxmltree`, `tiny-skia`, `resvg`/`usvg`,
`png`, `gif`, `jpeg-decoder`, `ab_glyph`, `jni`, `log`/`android_logger`,
`i_overlay` (pure-Rust polygon boolean operations: `ItemCoverage` hit testing
and translucent ink layering, replacing AWT `Area` / Android `Path.op`; approved
2026-11-02), `rustls` (ring provider, no default features) with
`rustls-pki-types`, `rustls-native-certs` (non-Android hosts) and
`webpki-roots` (fallback when no platform roots load) for `https://` peers;
`rcgen` as a core dev-dependency for TLS tests (approved 2026-11-02, ADR 0028).
Ask before adding others.

## Android porting patterns

Infrastructure in `apps/android/src` (verified on the x86_64 emulator):

- `jx` — JNI helpers. Every Java call is a macro naming the declaring class,
  method and JNI descriptor literally:
  `jcall!(view, "android/view/View", "setPadding", "(IIII)V", l, t, r, b)?`,
  `jnew!("android/widget/TextView", "(Landroid/content/Context;)V", &ctx)?`,
  `jstatic!("android/graphics/Color", "argb", "(IIII)I", a, r, g, b)?.int()`,
  `jget!(rect, "android/graphics/RectF", "left", "F")?.float()`,
  `jsget!("android/graphics/Paint$Style", "FILL", "Landroid/graphics/Paint$Style;")?.some()`,
  `jsuper!(this, "com/notemaker/NativeView", "superDispatchDraw", "(Landroid/graphics/Canvas;)V", &canvas)?`.
  Arguments convert with `From` (`&str`, `String`, `i32`, `f32`, `bool`, `i64`,
  `f64`, `&Obj`, `&JObject`, `Option<&Obj>`, `jx::NULL`). Results are `Ret`
  (`.int()`, `.float()`, `.bool()`, `.string()`, `.global()`/`.some()` → `Obj`).
  Use the exact Java overload (`setText(CharSequence)` is
  `(Ljava/lang/CharSequence;)V`). Constants that are compile-time `static
  final int`s in Java (e.g. `View.GONE`, `Gravity.CENTER`, `Color.BLACK`) are
  written as Rust constants with the same values in `android_consts.rs`.
- `apps/android/tools/check_jni.py` checks every literal against
  `android.jar` and the shim. Run it before every device build; it must pass.
- `Obj` is a global ref (Clone/Send/Sync). Java `==` on objects → `Obj::same`.
  Every helper that creates a local must delete it after promoting it to
  `Obj`, or the caller must use `jx::frame`. `Ret` deletes an ignored object
  result on drop. The instrumentation thread runs a whole suite inside one
  native `onStart` call, so a leaked local (for example the `int[]` from
  `bitmap_pixels`) stays reachable until the Java heap is exhausted.
  Locals (`jx::Local`) are only valid inside the current native call/frame;
  promote with `jx::global` to store. Long loops on Rust threads: wrap the
  body in `jx::frame(|| ...)`.
- Java exceptions → `Err(jx::JavaError)`; `e.is("java.io.IOException")`
  matches subclasses like a `catch`. Errors and panics escaping a native
  callback are rethrown into Java (`jx::guard`), so uncaught failures still
  crash the app like Java would.
- Listeners/lambdas: `jx::callback(|method_key, args| ...)` returns a
  `NativeCallback` implementing every listener interface (keys such as
  `onClick(View)`, `onClick(DialogInterface,int)`, `run()`), `jx::runnable`,
  `jx::try_runnable`. `NativeScaleListener` for `SimpleOnScaleGestureListener`,
  `NativeAdapter` for `BaseAdapter`.
- View subclasses and anonymous overrides: `NativeView`, `NativeLinearLayout`,
  `NativeFrameLayout` (`new(Context, long handle, int mask)`, mask bits
  DRAW=1, DISPATCH_DRAW=2, TOUCH=4, DETACHED=8, SIZE=16, ATTACHED=32; handle
  from a `jx::callback`-style closure; `super.x()` via `superOnDraw`,
  `superDispatchDraw`, `superOnTouchEvent`).
- Manifest components (`MainActivity`, `ReplicaService`, `Integration`) are
  shims calling `Native.component(this, event, args)`, routed by
  `components.rs` to `main_activity::component_event`,
  `replica_service::component_event`, `integration::component_event`.
  `MainActivity.handle` (a public `long`) stores `Arc::into_raw` of the Rust
  `MainActivity`; Rust code holding a Java activity reference recovers the
  Rust instance from that field.
- Fields: Java instance fields become `Field<T>` (reentrant per-field lock;
  `get`/`set`/`with`/`with_mut`), so struct values can be shared as
  `Arc<Self>` across the UI and worker threads. `synchronized(lock)` →
  `self.lock.enter()` on a `monitor::Monitor` (reentrant, `wait`/`notify_all`).
  Inner classes keep a `Weak`/`Arc` back-reference to their outer instance.
- `org.json` → `org_json::{JSONObject, JSONArray, JV}` with Java reference
  semantics and Android formatting. Keep the original round trips
  (`Json.parse(o.toString())`) verbatim; they change number types.
- Threads: `jx::JThread::spawn(name, f)` (cooperative `interrupt`,
  `JThread::sleep` → InterruptedException), `Handler`/`runOnUiThread` through
  JNI with `jx::runnable`. Time: `jx::nano_time`, `jx::current_time_millis`,
  `jx::uptime_millis`. Logging: `jx::log::{i,w,e}` (android.util.Log).
- Skeleton additions (see [`android-port-plan.md`](android-port-plan.md)):
  `error::{AppError, AppResult}` as the single error type; `concurrent`
  (`ExecutorService`, `Future`, `CountDownLatch`); `android_consts`;
  `android_canvas::AndroidCanvas` (the JNI `notemaker_cli::canvas::Canvas`);
  `ui` widget helpers; `jset!`, `jx::WeakObj`, `jx::native_view` /
  `native_linear_layout` / `native_frame_layout` / `scale_listener` /
  `adapter`, `jx::once_runnable`. Closures stored in Java objects capture
  owners weakly, and view structs name their own view with `WeakObj`.
  Global refs are GC roots, so this avoids uncollectable cycles.
