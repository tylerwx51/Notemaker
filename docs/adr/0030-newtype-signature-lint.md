# ADR 0030: Newtype signature lint

Status: accepted, 2026-11-03. Level: warn.

## Context

Non-private functions pass raw values such as `u32`, `&str`, `f64`, `bool`,
`serde_json::Value` and `Arc<T>`. Those types do not say what the value means,
and arguments of the same raw type are easy to swap. We want each such value
wrapped in a descriptive newtype owned by the crate. Clippy has no lint for
this, and a syntax-only checker cannot resolve imports or type aliases.

## Decision

- A Dylint library, `tools/lints/notemaker_lints`, defines the lint
  `raw_type_in_signature`. It runs inside rustc on resolved types, so aliases
  and `use .. as ..` renames are seen through.
- **Checked:** functions with any written visibility (`pub`, `pub(crate)`,
  `pub(super)`, `pub(in ..)`), including `pub` items in private modules,
  inherent methods, and methods of non-private traits. The check is syntactic
  because rustc resolves `pub(crate)` at the crate root to the same visibility
  as private. Private functions and `pub(self)` are not checked.
- **Flagged:** in parameters and the return type, primitives (`bool` in
  parameters, `char`, integers, floats, `str`), arrays, slices, non-empty
  tuples, and types from other crates, including `String`, `Vec` and
  `HashMap`. References, raw pointers, function pointers and the generic
  arguments of local types are checked through to their contents.
- **Allowed:** `Option<T>` and `Result<T, E>` when `T` and `E` pass. Std
  ownership and concurrency wrappers are transparent: `Box`, `Arc`, `Rc`,
  `Weak`, `Mutex` and its guard, `RwLock` and its guards, `Cell`, `RefCell`,
  `Ref`, `RefMut`, `OnceCell`, `OnceLock`, `LazyCell`, `LazyLock`, `Pin`,
  channel senders and receivers, `JoinHandle`, `ScopedJoinHandle`, `Condvar`
  and `Barrier`; their contents are still checked. Atomics and the default
  allocator are allowed. `bool` is allowed in return types, so predicates need
  no wrapper. Also allowed: `()`, `!`, generic parameters, `impl Trait`,
  `dyn Trait`, and types from crates in `allowed_crates` in the root
  `dylint.toml` (`notemaker_core`, `notemaker_cli`), so the CLI and Android
  app can pass shared newtypes.
- **Skipped:** trait impl methods (the trait fixes the signature), the crate
  entry point, and items from external macros.
- **JNI:** no `extern "system"` exemption. `#[no_mangle]` exports a symbol
  whatever its Rust visibility, so the JNI entry points become private shims
  that convert JNI values into domain newtypes and call checked `pub(crate)`
  functions.
- The library pins `nightly-2026-08-20` (the nightly that Dylint 6.3.2
  targets) and does not use `clippy_utils`. The NoteMaker crates stay on
  stable 1.95. `cargo-dylint` and `dylint-link` 6.3.2 are packaged in
  `tools/nix/dylint.nix` because nixpkgs does not have them.
- To exempt one item, use
  `#[cfg_attr(dylint_lib = "notemaker_lints", allow(raw_type_in_signature))]`.
  The root `[workspace.lints.rust]` registers the `dylint_lib` cfg, so the
  stable build does not warn about it. Every member crate opts in with
  `[lints] workspace = true`.
- **Java library ports:** helpers that reproduce general Java library
  semantics (`Math`/`StrictMath`, number formatting, `Json.java`, exception
  classes and `Model.require`, `File.getCanonicalPath`, `URLEncoder`,
  `org.json`, `java.util.concurrent`, monitors, `java.net.URI`,
  `Integer.parseInt`, `FileChannel.tryLock`) live in each crate's `java_port`
  module. The crate root exempts that module with the `cfg_attr` allow below,
  because these APIs take raw numbers, strings and JSON by design. A helper
  goes there only when more than one module uses it; a helper with one caller
  is a private function in that caller's module.
- **Newtype boundaries:** a newtype's own conversions go through public
  fields, struct literals and trait impls (`From`, `Deref<Target = str>`,
  `Display`), which the lint does not check, rather than inherent
  constructors or accessors that take or return the raw value.
  `notemaker_core::string_newtype!` defines string newtypes this way.
- The lint is introduced at **warn**. It does not run from `tests/run.py` or
  the pre-commit checks yet.

## Consequences

- The first run on 2026-11-03 reported 2,004 warnings: 537 in
  `notemaker_core`, 173 in `notemaker_cli` and 1,294 in `notemaker_android`.
  The most common types were `str`, `i32`, `f32`, `f64`, `bool`, `serde_json`
  maps and values, and `Arc<…>` UI handles. The four JNI exports in
  `apps/android/src/lib.rs` are now private `#[no_mangle]` shims that convert
  JNI values into checked types.
- 2026-11-03 revision: wrappers became transparent, `bool` returns and
  `notemaker_cli` types became allowed, and the Java helpers moved into
  `java_port`. That took the count from 2,004 to 1,356 before any newtype
  work.
- 2026-11-03 newtype pass (identity, geometry, colour/pages, time, Android/JNI
  boundary, flags/text/graphics boundary): 249 warnings remain, all in groups
  still under discussion: network and endpoint values, untyped JSON
  (`serde_json` maps and values, `org_json`), raw bytes/data URIs/SVG source
  and file paths, and std value types (`Path`, `PathBuf`, `Duration`,
  `Instant`, `ThreadId`, `process::Child`). Shared types live in
  `notemaker_core::{ids, geometry, style, clock}`, `notemaker_cli::canvas`,
  and the Android `jx`, `ui`, `android_consts` and `main_activity` modules.
- 2026-11-03 network pass: 224 warnings remain. `notemaker_core::net` holds
  `HttpUrl`, `Port` (`DEFAULT_SERVER` 8765, `DEFAULT_PROXY` 8766,
  `EPHEMERAL`), `BearerToken` (its `Debug` hides the value and it has no
  `Display`), the `HttpMethod` and `HeaderName` enums, `HeaderValue`,
  `HttpStatus` and `ChatId`. `EndpointPath` is an enum tree
  (`EndpointPath` → `ApiPath` → `UiPath`, `ChatPath`) that covers every
  request a client sends, `/command`, `/api/{info,replica,events,locks,stream}`,
  `/api/ui/*` and `/api/chats*`, with typed query parameters. It renders the
  exact targets the hand-built strings produced and names each request's
  method. `Endpoint::open`/`request` take it, and the per-path read timeouts
  are a `match` over it. The proxy parses forwarded chat requests into
  `ChatPath` with `server.py`'s routing and answers 404 `{"error":"Not found"}`
  itself for routes the server lacks. On Android, `network::ServerAddr` and
  `ProxyAddr` are enums (`Unset` or `Set(HttpUrl)`) instead of empty strings,
  with `normalize`/`compose`/`from_input` constructors that keep the Java
  `URI` behaviour. The settings fields arrive as a `NetworkSettingsForm`. The
  endpoint descriptor (`Endpoint::new(Map)`, `Endpoint::resolve`,
  `LocalProxy::configure` targets), replica lineage IDs, change generations,
  connection timeouts and `Endpoint::cancel(ThreadId)` are outside this pass.
- Moving to `deny` requires fixing or explicitly allowing each crate's
  warnings, then adding the lint to the check runner.
- Bumping the pinned nightly can break the lint, because rustc internals are
  unstable. Update Dylint, the nightly and the lint together.
- The return types of `async fn` are opaque futures and are not inspected. No
  NoteMaker crate uses `async fn`.
