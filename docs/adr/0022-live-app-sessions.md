# ADR 0022: Live app sessions and semantic CLI controls

Status: superseded by [ADR 0023](0023-networked-ui-sessions.md), 2026-10-21.

## Context

The CLI already edits document content through a local replica, but document
navigation, tool choices, the Files browser, and view controls live in the
Android Activity. Changing saved editor preferences alone does not guarantee
that the running screen changes immediately. The current Files browser is a flat
document list; it has no folder hierarchy.

## Decision

Create a fresh UUID when the Activity launches. Keep it while the Activity moves
between foreground and background, and unregister it when the Activity is
destroyed. The local authenticated proxy holds live session handlers in memory.
It does not write screen commands to the document journal or forward them to the
main server. CLI callers use the tablet's local proxy descriptor, with ADB or
SSH forwarding when calling from another computer. The selected editing proxy
continues to handle document content edits.

Expose `ui sessions`, `ui state SESSION`, `ui actions SESSION`, and
`ui action SESSION NAME FILE|-`. The Android `UiActions` registry defines stable
semantic names and JSON arguments and runs handlers on the Activity main thread.
Responses include the resulting screen state. A missing or closed session fails
explicitly. Content creation and edits remain shared local-state CLI commands;
the UI actions invoke the same Activity paths as visible controls.

Every new navigation or editor control must be registered as a semantic action
and verified through an actual CLI invocation. Keep the registry discoverable
through `ui actions`. The flat Files browser remains the current document model;
adding folders requires a separate document and UI design.

## Verification

Build JVM CLI and APK in the Nix shell. Run `tests/ui_sessions_android.py` to
exercise CLI session discovery, Files navigation, document and page changes,
tool controls, and session renewal after Activity restart on API 36. Physical
E Ink refresh and BOOX-specific behavior still need hardware checks.

## Update 2026-11-02

The Java implementation described here was ported one-to-one to Rust and
removed (ADR 0026 CLI, ADR 0029 Android app, `docs/design/rust-port.md`).
The decision stands; JVM, `app_process`, jsvg/androidsvg and `.java` details
now refer to their Rust equivalents (native CLI binary, resvg).
