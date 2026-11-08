# ADR 0025: Keep emulator pre-commit checks within five minutes

## Status

Accepted.

## Context

The pre-commit hook builds the CLI and APK and runs seven emulator suites plus
local checks. The original emulator runs repeatedly launched instrumentation,
force-stopped the app, restarted it, and read the accessibility tree through a
separate UI Automator process. The Android CLI suite alone spent about 210
seconds in 52 instrumentation launches, 91 seconds in 52 app restarts, and 39
seconds in 17 tree scans. Cold emulator setup and APK installation added roughly
30–42 seconds to each suite. Full hook runs exceeded 18 minutes.

The strict performance suite must run without another emulator: even a single
concurrent file-browser emulator produced stroke latencies over its 30 ms gate.
Five software-rendered emulators also consumed too much host memory and made UI
checks unreliable.

## Decision

Use a test-only loopback instrumentation bridge to issue multiple structured
commands to one running app process. The bridge supplies the real Android
accessibility tree and keeps explicit restart cases. Only generate large render
images or check cache parity for actions that request those outputs. Retrieve
viewport diagnostic images only when the viewport assertion fails.

Use host GPU rendering with Vulkan disabled for automated x86_64 emulator
suites. Keep the normal launcher defaults for manual use and allow test GPU
selection through `NOTEMAKER_TEST_GPU`. Give integration two scheduler slots;
start the strict performance check first and alone, then the long drawing and
viewport suites in the first functional batch; start the file-browser suite at
the next free slot; allow up to five emulator slots and six worker threads so
local suites can overlap the functional emulators; give the file-browser suite
its own port. Space its input samples six milliseconds apart
while retaining the same stroke count and strict per-event latency limit.
Render 40 untimed pen strokes in a disposable document before the measured
document: the host GPU occasionally spends over 30 ms issuing a command during
its first few pen frames. The measured document still contains exactly 1,000
strokes, and every event after its existing ten-stroke warmup retains the same
30 ms gate.

## Consequences

Tests still cover real Android UI input, two-client synchronization, offline
restarts, pixel comparisons, viewport cache behavior, and every performance
event. Host GPU capacity affects the default five-slot schedule; lower `--jobs`
or set `NOTEMAKER_TEST_GPU` when running on a different development machine.
The bridge is confined to instrumentation code and binds only to loopback in
the guest. The normal application and production CLI do not expose it.

The full pre-commit hook passed all 22 checks in 292.23 seconds on this host
with six workers and five emulator slots. See `docs/verification.md` for the
suite timings and remaining hardware limits.

## Update 2026-11-07

Headless runs with `NOTEMAKER_TEST_GPU=swiftshader_indirect` exposed three
harness defects:

- With `--jobs 1` the emulator limit was one slot, so integration (two slots)
  never started and the runner waited forever. A suite that needs more slots
  than the limit now runs when no other emulator is active.
- Suites installed with a plain `adb install -r`. Because the build writes a
  v4 signature (`.idsig`), API 30+ installs incrementally: `adb` reports
  success and then streams the APK in the background. Under CPU load the app
  could load `libnotemaker.so` (or `cli_android` could run the packaged CLI)
  before its pages arrived (`file offset ... >= file size: 0 >= 0`). Suites
  now use a full `--no-incremental` install, and only after
  `sys.boot_completed`, `RUNNING_UNLOCKED` user storage and a responding package
  manager.
- An emulator that never registers with ADB makes `adb -s SERIAL logcat`
  wait indefinitely, and QEMU in that state ignores SIGTERM. `chat_ui` raised
  out of its cleanup and left QEMU, its crash handler and the mock chat server
  running. `tests/android_emulator.py` now owns emulator start, boot wait,
  install, diagnostics and cleanup for every emulator suite. Every ADB call has a
  timeout, and stop escalates to SIGKILL for QEMU. It signals only QEMU: the
  first emulator spawns the per-user `netsimd` that all running emulators share,
  so killing an emulator's process group breaks the others.

Two load-dependent test races were fixed: `chat_ui` now waits for the sent turn
to leave `idle` before requiring `done` (Send posts asynchronously), and
`cli_android` waits for the force-stopped app process to exit, and so release
`proxy.lock`, before starting its lock holder. Verification under the same load
found three more: text typed before the IME bound to a newly focused field
could repeat its first key (`TToolbar-text`), so suites type only after
`mInputShown=true`; `drawing_android` read the state before the background
fill command finished; and `files_android` tapped a still-opening dropdown a
second time, which closed it.
