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
- **Flagged:** in parameters and the return type, primitives (`bool`, `char`,
  integers, floats, `str`), arrays, slices, non-empty tuples, and types from
  other crates, including `String`, `Vec`, `Box`, `Arc` and `HashMap`.
  References, raw pointers, function pointers and the generic arguments of
  local types are checked through to their contents.
- **Allowed:** `Option<T>` and `Result<T, E>` when `T` and `E` pass. Also
  allowed: `()`, `!`, generic parameters, `impl Trait`, `dyn Trait`, and types
  from crates in `allowed_crates` in the root `dylint.toml`
  (`notemaker_core`), so the CLI and Android app can pass core newtypes.
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
- The lint is introduced at **warn**. It does not run from `tests/run.py` or
  the pre-commit checks yet.

## Consequences

- The first run on 2026-11-03 reported 2,004 warnings: 537 in
  `notemaker_core`, 173 in `notemaker_cli` and 1,294 in `notemaker_android`.
  The most common types were `str`, `i32`, `f32`, `f64`, `bool`, `serde_json`
  maps and values, and `Arc<…>` UI handles. The four JNI exports are flagged
  until they are made private.
- `notemaker_cli` types, such as `canvas::RectF`, are flagged in
  `notemaker_android`. Add `notemaker_cli` to `allowed_crates` if the app should
  share them.
- Moving to `deny` requires fixing or explicitly allowing each crate's
  warnings, then adding the lint to the check runner.
- Bumping the pinned nightly can break the lint, because rustc internals are
  unstable. Update Dylint, the nightly and the lint together.
- The return types of `async fn` are opaque futures and are not inspected. No
  NoteMaker crate uses `async fn`.
