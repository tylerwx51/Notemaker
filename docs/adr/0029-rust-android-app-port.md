# ADR 0029: Rust Android app over JNI; Java implementation retired

Status: accepted, 2026-11-02. Completes the port begun in ADR 0026 (CLI) and
`docs/design/rust-port.md` (core).

## Context

NoteMaker's core, CLI and Android app were Java. The goal was a one-to-one
Rust port that keeps behavior, persisted bytes, wire formats, CLI output and
test entry points, so the physical tablet upgrades in place and the existing
Python suites keep driving the app.

Android requires manifest components (`Activity`, `Service`,
`Instrumentation`) and `View` subclasses with overridden callbacks. JNI cannot
subclass Java classes, and reimplementing the UI without Android widgets
would not be one-to-one.

## Decision

- The app is the Rust crate `notemaker_android` (`libnotemaker.so`). It calls
  the same Android framework APIs the Java code called (widgets, `Canvas`,
  `Bitmap`, dialogs, `Handler`) through JNI (`apps/android/src/jx.rs`).
- A thin Java shim (`apps/android/shim/com/notemaker/`) keeps the manifest
  class names and adds generic forwarding classes (`NativeCallback`,
  `NativeView`, `NativeLinearLayout`, `NativeFrameLayout`, `NativeAdapter`,
  `NativeScaleListener`). It holds no application logic.
- Every JNI call names its class, method and descriptor literally;
  `apps/android/tools/check_jni.py` verifies them against `android.jar` and the
  shim before device builds.
- Java instance fields become `Field<T>` (reentrant per-field lock) and
  `synchronized` becomes a Java-compatible `Monitor`. Android `org.json` is
  ported with reference semantics and its number formatting (`org_json.rs`).
- The CLI runs on Android as the same native binary, packaged as
  `lib/<abi>/libnotemaker_cli.so` and executed from `nativeLibraryDir`
  (`android:extractNativeLibs="true"`), replacing `app_process`.
- SVG objects render with resvg on all platforms (replacing jsvg and
  androidsvg). The APK bundles a generated third-party crate license notice.
- The Java sources were removed from the tree. The last Java commit,
  `0534499`, is the reference; `packages/core/tests/parity.sh` extracts it
  from git to compare outputs and persisted bytes.

## Consequences

- The signing key and package name are unchanged, so installed tablets
  upgrade in place with their replicas intact (verified with
  `tests/tablet_checks.py --install`: all files preserved byte-for-byte, actor
  histories unchanged).
- Known, documented deviations: resvg sub-pixel coverage differs from Skia
  for tiny shapes (two coverage thresholds widened); failures that crashed the
  Java process in a few background paths are logged instead; SVG objects are
  rasterized per uncached draw rather than cached as `Picture`s.
- Java reference semantics that the port first replaced with deep copies were
  later restored as copy-on-write (ADR 0027) after emulator and tablet
  performance runs.
- JDK 17 stays in the Nix shell for the shim (`javac`, `d8`, `keytool`) and
  for the parity harness.
