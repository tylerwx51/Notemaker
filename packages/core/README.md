# notemaker_core (Rust port of the shared Java core)

`packages/core` holds the shared replica, action model and rendering traversal.
This is the one-to-one Rust port (`src/*.rs`, crate `notemaker_core`) of the
former Java core; the Java sources were removed and live in git history at
commit `0534499` (ADR 0029). Conventions are binding and documented in
[`docs/design/rust-port.md`](../../docs/design/rust-port.md).

## Build and test

```sh
nix-shell --run 'cargo build -p notemaker_core && cargo test -p notemaker_core'
nix-shell --run 'cargo ndk -t arm64-v8a -P 26 build -p notemaker_core'
packages/core/tests/parity.sh                    # full Java/Rust parity run (needs the JDK)
packages/core/tests/parity.sh --update-snapshot  # also refresh the committed Java snapshot
```

`cargo test` includes `tests/parity_snapshot.rs`. That test compares the Rust
output with the committed Java snapshot, so it needs no JDK. It also reads a
replica directory written by the Java CLI (`tests/parity/fixtures/state-java`).

## Java class → Rust module

| Java | Rust module | Main items |
| --- | --- | --- |
| `Json` | `json` | `Value`, `Map`, `List`, `object!{}`, `parse`, `write`/`write_map`/`write_list`, `copy`, `equivalent`, `map`/`list`/`string`/`number` (+`*_at`, `*_mut`), `put_if_absent`, `get_or_default` |
| `Timing` | `timing` | `set_observer(Option<Box<dyn Fn(&str)>>)`, `mark` |
| `Geometry` | `geometry` | `Polygon = Vec<[f64;2]>`, `rect`, `bounds`, `contains`, `intersects`, `polygon`, `json`, `capsule` |
| `InkGeometry` | `ink_geometry` | `InkGeometry` |
| `Coverage`, `Coverage.Factory`, `Coverage.Tiles`, `Tiles.Painter` | `coverage` | traits `Coverage`, `CoverageFactory`, `Painter`; struct `Tiles` |
| `GifAsset` (+`Info`) | `gif_asset` | `validate(Option<&str>)`, `validate_bytes(Option<&[u8]>)`, `from_bytes`, `Info` |
| `ImageAsset` | `image_asset` | `validate(Option<&str>) -> [f64;2]`, `from_bytes` |
| `SvgAsset` (+`DotGrid`) | `svg_asset` | `validate -> [f64;2]`, `viewport`, `dot_grid -> Option<Arc<DotGrid>>` |
| `Endpoint` (+`HttpError`) | `endpoint` | `Endpoint`, `HttpConnection`, `BodyReader`, `Disconnector`; `HttpError` is `ErrorKind::Http` with `Error.status` |
| `Actions` (+`Build`) | `actions` | `id`, `drawing`, `diagram`, `validate_references`, `inverse`, `type Build` |
| `LayerEditor` | `layer_editor` | `page`, `LayerEditor::{new, with_inactive, for_erase, items, resolve, resolve_refs, require_references, select, select_in}` |
| `ShapeCatalog` | `shape_catalog` | `ids`, `known`, `json`, `name`, `polygon`, `vertices`, `drawing_svg` |
| `RegionFill` | `region_fill` | `create` |
| `DrawItem` | `draw_item` | `DrawItem` |
| `Collaboration` | `collaboration` | `LEASE_MILLIS`, `require_selection`, `key`, `expire`, `conflicts`, `affected`, `require_unlocked`, `require_incoming_unlocked`, `transform`, `transform_simple`, `now_millis` |
| `CheckpointPointPrefixes` | `checkpoint_point_prefixes` | `encode`, `decode` (crate-internal use by `journal`) |
| `DrawingActions` | `drawing_actions` | `copy_selection`, `paste`, `duplicate_layer`, `merge_layer`, `reorder_layer`, `image`, `template`, `fill`, `erase`, `erase_regions` |
| `Model` | `model` | `Model`, `Entity = Arc<Map>`, `model::{drawing, color, range, id, bool_}` |
| `Store` (+`Snapshot`, `Edit`, `UploadGate`) | `store` | `Store`, `State`, `StateKind`, `store::model(&State)`, `model_of_map`, `url_encode`, `type UploadGate` |
| `Journal` (+`State`) | `journal` | `access`, `checkpoint`, `canonical_path`, `type Edit` |
| `Render` (+`Surface`, `Svg`) | `render` | `trait Surface`, `Svg`, `draw`, `draw_item`, `draw_stroke`, `draw_object`, `endpoints` |
| — | `error` | `Error { kind, message, status }`, `ErrorKind`, `Result`, `require`, `require_with` |
| — | `jfmt` | JDK 17 `Double.toString`/`Float.toString` (`double`, `float`, `append_double`), `parse_double`, `hex_color`, `base64_encode`/`base64_decode` (Java messages), `java_trim` |
| — | `jmath` | fdlibm `atan2`, `atan`, `acos`, `hypot`; Java `round`, `rint`, `to_radians`, `min`/`max`, `floor_div`, `d2i` |
| — | `ids` | `random_uuid`, `name_uuid_from_bytes` |

## Signature mappings

- **Exceptions** become `Result<T, Error>`. `error::ErrorKind` names the Java
  class (`IllegalArgument`, `Io`, `Http`, `ClassCast`, `NullPointer`, …).
  `Error::message()` is the exact Java message; `Error::status` carries
  `HttpError.status`. Unchecked casts of JSON data (`(Number) x`, `(Boolean) x`)
  return `ClassCast`/`NullPointer` errors instead of throwing. Their messages
  are not the JVM's.
- **JSON values**: `Map<String,Object>` is `json::Map` (insertion ordered) and
  `List<Object>` is `Vec<Value>`. Java `Integer`/`Long` are `i64` numbers and
  `Double` is `f64`. Nullable `String` lookups use `Option<&str>`.
- **`double[]` / `double[][]`** are `[f64; N]` or `Vec<[f64; 2]>`
  (`geometry::Polygon`).
- **Model maps**: `layers`/`strokes`/`objects` are `IndexMap<String, Entity>`.
  Here `Entity = Arc<Map>` is the immutable snapshot that model copies share;
  `Model::clone()` is `new Model(other)`. `active`/`object_active`/
  `layer_active`/`conflict_active` are `IndexMap<String, bool>`. Java field
  `type` is `type_`. Constructors: `Model::new()`; `Model::from_state(&Map)` is
  `new Model(Map state)`; `Model::from_json(&Map)`. `Model::with_bases(&mut Map, &Model)`.
- **`Render.Surface`** is the object-safe `trait Surface`, taking
  `&mut dyn Surface`. Default methods match the Java defaults, including the
  `UnsupportedOperation` errors. Methods return `Result<()>`, and Java's
  `try/finally` around erase masks and transforms is preserved.
  `Render.Svg.toString()` is `Display` (`svg.to_string()`).
- **`Store.Edit`** is `&mut dyn FnMut(&mut State) -> Result<()>`.
  **`Actions.Build`** is `&mut dyn FnMut(&mut Model) -> Result<Vec<Value>>`.
  **`Store.UploadGate`** is `&mut dyn FnMut() -> Result<()>`.
- **`Store` state maps**: Java returned a `Journal.State` (a map carrying the
  model). Rust returns `store::State`, which derefs to `json::Map` and carries
  the materialized model; `State::snapshot(&model)` replaces
  `new Store.Snapshot(model)`. Functions that took `Map state` and called
  `Store.model(state)` (`Collaboration::require_unlocked`,
  `require_incoming_unlocked`) take `&mut State`. Those that only touch the
  map take `&mut Map` or `&Map`.
- **Overloads**:
  - `commit(actor, build)` → `commit`
  - `commit(actor, build, deps)` → `commit_with_dependencies`
  - `commitWithState` → `commit_with_state -> (Map, State)`; Java put the
    state under `result["state"]`
  - `history(actor, redo, true)` → `history_with_state -> (Map, State)`
  - `Collaboration.transform(m, e, scale, dx, dy)` → `transform_simple`
  - `GifAsset.validate(byte[])` → `validate_bytes`
  - `LayerEditor` constructors → `new` / `with_inactive` / `for_erase`
  - static `LayerEditor.select` → `select_in`
  - `LayerEditor.resolve(List)` → `resolve_refs`
  - `DrawItem.ref()` → `ref_()`
  - `Model.bool` → `bool_`
- **`Timing.observer`** (a `ThreadLocal<Consumer<String>>`) is
  `timing::set_observer`, which is per thread and returns the previous one.
- **`Coverage.Factory` identity**: `DrawItem` caches coverage per factory
  address, which matches Java's `factory != coverageFactory` check.
- **`Endpoint.open`** returns `HttpConnection` (lazy request on
  `response_code()`, `input_stream()` / `error_stream()` readers,
  `disconnector()` for cancelling from another thread).
  `HttpConnection::set_tls_config` / `Endpoint::with_tls_config` replace
  `HttpsURLConnection.setSSLSocketFactory` (custom roots, used by tests).
  `cancel(Thread)` is `cancel(ThreadId)`.

## Platform numerics

- `Double.toString` on JDK 17 and Android is OpenJDK's `FloatingDecimal`, which
  is **not** shortest round-trip (for example `1e23` → `9.999999999999999E22`).
  `jfmt` ports it exactly.
- `Math.atan2`/`acos`/`hypot` are fdlibm on every Java runtime and are ported.
  They were bit-exact against HotSpot on 200,000 random inputs.
- `Math.sin`/`cos` are runtime intrinsics. The port calls the platform libm, so
  on Android Rust and ART both use bionic. On the JVM, HotSpot's x86 stubs
  differ from glibc by 1 ulp for about 0.1% of inputs. The parity harness
  tolerates those, and they only appear in a few `geometry/capsule` lines.

## Parity harness (`tests/parity/`)

- `make_fixtures.py` generates fixtures deterministically: JSON cases, invalid
  operations, raster assets, scenes covering every brush and material, every
  shape, text, images/GIFs/SVGs, erasures, connectors, layers and conflicts,
  plus drawing-action calls.
- `fixtures/svg/` holds 34 good and 36 bad SVG inputs.
- `Parity.java` (compiled against the Java sources, which `parity.sh` extracts
  from commit `0534499`) and `driver.rs` (example
  `parity`) print the same canonical sections, with UUIDs canonicalized.
- `make_state.sh` drives the real JVM CLI through a proxy to produce a
  Java-written replica.
- `parity.sh` compares the following (`compare.py` diffs, tolerating only
  float ulps and the entries in `expected/known-differences.txt`):
  1. All fixture sections.
  2. A deterministic write sequence covering `accept`, `append`, `commit`,
     undo/redo, settings and list splices, leases, selection commit,
     prefix-compressed and relay-ids checkpoints, conflicts and delete. The
     full base file and every journal record (length, CRC, JSON) are compared.
  3. Each implementation reading the other's written directory.
  4. Java and Rust reading the CLI-written replica.
  5. Rust appending to the Java replica; then Java appending to the result.
     Both implementations read each stage.

### Known differences

- Rust `Value` cannot hold NaN/Infinity, so a non-finite double becomes `null`.
  Java kept it and threw `Nonfinite JSON number` on write. Model validation
  rejects such values with the same message either way.
- XML well-formedness errors in `SvgAsset` carry roxmltree wording after the
  `Invalid SVG: ` prefix. Xerces wording is not reproduced. All of SvgAsset's
  own messages match.
- Java distinguishes `Integer` from `Long` (and Java `Float` from `Double`) in
  `equals`. Rust compares integer values numerically. This can change the rare
  Java behaviors that depended on boxed type identity:
  - a journal `meta` entry where Java saw `Integer 0` ≠ `Long 0`;
  - `Collaboration.expandLayer`'s `order` comparison after an in-process
    `reorderLayer`.

  Persisted bytes for the same operations were identical in every harness run.
- Lone UTF-16 surrogates from `\uD800`-style JSON escapes become `?` at parse
  time. Java kept them in memory and emitted `?` on UTF-8 encoding, so the
  bytes are the same but in-memory comparisons differ. `Integer.parseInt`'s
  acceptance of non-ASCII Unicode digits in `\u` escapes is not reproduced.
- `Model.fromJson` rejects `null` active flags and versions, which Java stored
  silently.
- Endpoint:
  - `https://` uses rustls (ring) instead of JSSE/Conscrypt, trusting the
    platform CA store plus `NOTEMAKER_EXTRA_CA_FILE` (`tls` module, ADR 0028).
    Handshake failures are `ErrorKind::SslHandshake` /
    `SslPeerUnverified` with the JDK (host) or Conscrypt/OkHttp (Android)
    messages for untrusted issuers, hostname mismatches and expiry; other
    TLS errors carry rustls' text. No session resumption across requests,
    no client certificates, no `javax.net.ssl.trustStore` properties.
  - Redirects are not followed.
  - Requests send `Connection: close` without Java's default
    `User-Agent`/`Accept` headers.
- `Store`/`Journal` snapshots deep-copy JSON state, where Java shared op maps
  between snapshots. Reads therefore copy the whole event log; this is a
  performance difference, not a format difference.
- Nested store or journal access from inside an edit callback on the same
  thread returns `OverlappingFileLock`, as Java's second `FileChannel.lock()`
  did, instead of deadlocking.
