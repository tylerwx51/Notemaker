# ADR 0026: Port the CLI and embedded proxy to Rust

## Status

Accepted. This ADR supersedes the JSVG part of ADR 0005 for the CLI.

## Context

The repository is being ported one-to-one from Java to Rust
([docs/design/rust-port.md](../design/rust-port.md)). The CLI had a JVM
variant (AWT, `Area`, jsvg) and an Android variant (`android.graphics`,
`Path.op`, androidsvg). Each variant had its own `Png` and `ItemCoverage`. The
Android app embeds `LocalProxy` and draws through `Png(Canvas)` and
`Png.StrokeCommands`, so the Rust CLI library must also serve the app's port.

## Decision

`apps/cli` becomes the crate `notemaker_cli` (library plus `notemaker` binary).
The crate includes the `tests/*.java` probes as extra binaries. The public API
mirrors the Java classes; see [apps/cli/RUST.md](../../apps/cli/RUST.md).

- **One rendering implementation.** `Png` renders through a backend-neutral
  `canvas::Canvas` trait, a subset of `android.graphics.Canvas`. tiny-skia
  (`RasterCanvas`) is the CLI backend. The Android app supplies a JNI backend.
  `StrokeCommands` is plain data, replayable on any `Canvas`.
- **SVG and text.** SVG objects use resvg. Text uses ab_glyph outlines from
  DejaVu (via `FONTCONFIG_FILE`) on the host and from `/system/fonts` on
  Android.
- **Boolean geometry.** `ItemCoverage` hit testing and translucent ink layering
  use `i_overlay`. Paths are resolved once into oriented shapes, then tested
  for positive-area overlap, as with `Area`.
- **Proxy runtime.** The proxy keeps Java's HTTP handling, thread names,
  `fcntl` ownership lock and output bytes. The JVM shutdown hook becomes a
  SIGINT/SIGTERM/SIGHUP handler in `serve`.
- **Build.** `apps/cli/build.sh` runs `cargo build --release`. `--android`
  cross-builds arm64-v8a and x86_64 with cargo ndk.

## Consequences

- The CLI no longer needs a JVM. The Java sources stay until the Android app
  port lands; `packages/core/tests/parity.sh` compiles the Java CLI itself.
- Exported-SVG pixel tests carry small resvg tolerances, and text hit testing
  depends on the chosen font. Both are recorded in `docs/verification.md`.
- A few Java exception class names cannot be represented by
  `notemaker_core::Error`, for example `NoSuchFileException`. Messages are
  kept, and the closest kind is displayed.
