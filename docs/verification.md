# Verification

Executed locally on 2026-10-09–10 through the root Nix development environment.

## Commands

```sh
nix-shell --run 'apps/android/build.sh'
nix-shell --run 'python3 tests/server_checks.py'
nix-shell --run 'python3 tests/integration.py'
```

The Android APK built and signed successfully. Compiler output includes Java 8
bootstrap and deprecated Android inset API warnings; neither prevents the build.
The three server protocol checks passed: document isolation/cursors/idempotency,
transaction rollback on conflicting operation IDs, and rejection of invalid
pressure without partial batch insertion. Python source compilation and
`git diff --check` also completed successfully.
The rebuilt final APK passed `apksigner verify` and was installed and launched
for a separate visual check. Its system-bar icons, drawing controls, saved canvas,
and offline status were visible; `.test-artifacts/android-ui-final.png` records
that check. The preview emulator was stopped afterward.

## Two-emulator integration

The complete integration suite finished successfully with
`ALL INTEGRATION CHECKS PASSED` and exit code 0.

The integration harness starts the actual server and two independent API 36
Google APIs x86_64 emulators simultaneously, each with a 1860 × 2480 display,
two virtual CPU cores, and a requested 1,536 MiB guest RAM. The current emulator
raises that memory allocation to 3,072 MiB per guest. Both use the host server through
`10.0.2.2:18765`. The standard BOOX launcher and normal AVD data are preserved.

Instrumentation drives the actual Activity and Canvas with pressure-bearing
stylus/finger MotionEvents. Ordinary app processes continue running between test
commands. Convergence checks read their atomically saved production state;
comparing it does not restart the apps or interrupt their retry loops.

The harness exercises:

- Simultaneous color/pressure strokes from both clients; identical committed
  event histories and byte-identical normalized rendered PNGs.
- Idempotent replay of a previously committed upload.
- Local layer hiding, shared forced visibility, and restoration of local hiding
  when force is released.
- Shared undo/redo, whole-stroke erasure, and undo of erasure.
- Drawing from both clients while the server is stopped, offline queue recovery
  after force-stopping an app, server restart from SQLite, and automatic merging
  of all five distinct strokes without loss.
- Headless Chromium reading the same document log and enforcing force visibility.
  Browser/native colored-pixel coverage must differ by less than 10%; rendered
  bounds must agree within two pixels. Different rasterizers may antialias edges
  differently. Android-to-Android normalized PNG comparisons are exact.
- Multipage and infinite document sync, and mixed finger/stylus drawing.
- A system-injected eight-second finger gesture whose partial stroke must appear
  in the second running client's saved event log before finger-up.
- A fresh local open of an existing multipage document with the default
  fixed-page creation choice, retaining the server document's original type.

Ignored artifacts in `.test-artifacts/` include `results.txt`, emulator/server
logs, client state JSON, normalized PNGs, `browser-canvas.png`, browser coverage
statistics, and `android-ui.png`. Test-owned processes stop on completion.

## Remaining hardware checks

No physical BOOX tablet was connected. Real E Ink refresh/ghosting, pen latency,
pressure calibration, palm behavior, proprietary BOOX firmware behavior, and
Wi-Fi access through the desktop firewall remain unverified. Emulator injection
checks pressure handling in application code rather than physical pen sensing.
The app uses standard Android Canvas/input APIs; no BOOX-specific SDK is included.
No deployment to the home server was performed.

## Sidebar and layer tray revision — 2026-10-10

The redesigned APK built successfully. `tests/ui_review.py` passed on the API 36
emulator using actual Android UI controls. It verified pen color selection,
document/page/tool popups, adding layers, local visibility, shared force visibility
and its release, undo/redo, and saving the tray expansion preference across an
app restart. Canvas bounds were identical before and after collapse/expand;
tapping the empty tray did not create a stroke underneath it.

The full `tests/integration.py` regression suite also passed with the redesigned
APK and two emulators running simultaneously against the local server. This
covered identical shared images, live partial strokes, layer visibility,
undo/redo, erasing, offline recovery, browser rendering, and document types.
The harness finished with `ALL INTEGRATION CHECKS PASSED` and exit code 0.

Expanded/collapsed states and the pen/layer popups were visually reviewed. Layer
previews reserve space for their numbers and enforce a minimum preview line width
to keep small strokes visible. Screenshots are in `.test-artifacts/ui-review/`:
`editor-final.png`, `collapsed-layers.png`, `pen-popup.png`, and `layer-popup.png`.

## Local replicas, CLI, and diagram objects — 2026-10-11

Built the JVM CLI, standalone Android DEX jar, and signed APK through `nix-shell`:

```sh
nix-shell --run 'apps/cli/build.sh --android && apps/android/build.sh'
nix-shell --run 'python3 tests/cli_checks.py'
nix-shell --run 'python3 tests/cli_android.py'
nix-shell --run 'python3 tests/integration.py'
```

The 11 CLI/server checks passed. These execute the actual JVM CLI, including 24
concurrent writers sharing one replica, actor-specific history, whole-diagram
undo/redo, PNG/SVG export, rejected invalid actions without partial writes,
connector validation, offline replicas resolving conflicting moves/erasure,
1,100-event pagination, and migration of the old Android state format. A controlled
HTTP proxy verifies that edits during a sync request remain pending and that a
lost upload acknowledgement can be retried without duplicating committed events.
An unrelated server identity is rejected before pending edits are uploaded.

The Android CLI suite passed on API 36. It launched the compiled CLI as an actual
child of the NoteMaker app, under the app's UID, while the app drew a pen stroke
without a server. It verified local visibility, actor undo/redo, labeled PNG
export, erasing an object through the object editor and undoing it through the
human UI, and observing those UI changes through the CLI. It then synchronized
with a desktop CLI replica, restarted the real server after offline edits,
verified identical effective models and a moved-but-erased object, checked browser
rendering, and verified persistence after restarting the app.

During verification the Android PNG adapter needed explicit font initialization
in the standalone child. It now builds a minimal font map from a readable font;
this does not affect the normal app's system font map. The PNG and object editor
were visually reviewed. A reconnect wait that repeatedly instrumented/restarted
the app was timing-sensitive; convergence polling now reads the durable replica
without restarting the process, following the existing integration harness.

The complete existing two-emulator regression also passed with
`ALL INTEGRATION CHECKS PASSED`: live partial strokes, color/pressure, independent
layers and forced visibility, erasure/undo/redo, offline and process recovery,
browser coverage, multipage and infinite documents all remain working. Python
source compilation and `git diff --check` passed. The final APK and both CLI
artifacts compile; the existing Java 8 bootstrap/deprecated API warnings remain.

Artifacts include `.test-artifacts/cli-diagram.png` (created by the desktop CLI),
`.test-artifacts/cli-android/android-cli.png`, `object-editor.png`,
`browser-canvas.png`, and `latest-state.json`, plus the ordinary integration logs.
No personal emulator data was modified; test-owned emulators and servers stopped.

Physical BOOX execution, E Ink behavior, and Phenom II runtime/deployment were not
tested. Android child PNG initialization uses isolated non-public framework font
methods; verify this on BOOX firmware. The CLI is JVM/DEX bytecode and requires
its platform runtime, rather than being a standalone native executable. No agent
orchestration or remote deployment was added.

## Physical BOOX latency and ANR investigation (2026-10-12)

Built the current working tree through `nix-shell --run 'apps/android/build.sh'`,
installed it on the USB-connected Note Air6 C (Android 16 / API 36), and confirmed
`com.notemaker/.MainActivity` launched. Subsequent manual drawing exposed severe
latency and an Application Not Responding (ANR) failure.

Read-only ADB collection through `nix-shell` captured logcat, historical exit
information, DropBox ANR traces, and a copy of the private document files under
ignored `.test-artifacts/boox-diagnostics/`. Tablet log times show frames taking
726 ms, 2,076 ms, 3,052 ms, and eventually 10,823 ms. Two ANR traces report input
timeouts of 5,000 ms and 9,504 ms; exit-info records termination after the ANR
dialog at 17:05:29.940, with reason `ANR` and description `user request after
error`.

Both ANR stacks place the main thread inside `String.matches` called by
`Json.Parser.value`, reached through `Store.locked → Store.edit →
MainActivity.save → enqueue → CanvasView.emit → onTouchEvent`. The local-state
thread is waiting for the activity lock held by the main thread. This directly
identifies synchronous document parsing during pen input as a cause of the
stall. The numeric parser constructs regex machinery for every number.

Code inspection also shows that every emitted stroke snapshot appends to the
document, rewrites and fsyncs the entire state, rebuilds the model, then repeats
the state read/write/rebuild to save preferences. Pen-up performs another save.
Full stroke snapshots accumulate while drawing, increasing subsequent work.
The captured 280,947-byte document contains 12 distinct strokes represented by
52 stroke snapshots (8,522 stored point entries versus 2,214 in the latest
snapshots), with 53 pending operations and no synchronized events. A background
loop also rereads the document every 200 ms under the activity lock.

Sync attempts time out against the emulator default `10.0.2.2:8765`; a physical
tablet needs a reachable configured server address for synchronization. The
network request runs on the sync thread outside the state lock and is not the
main-thread call captured in either ANR. Offline drawing must remain responsive.

These findings establish application-side input stalls; they do not quantify
additional E Ink refresh or stylus latency. No application fix or new APK was
applied during this investigation. The captured document remains on the tablet.

## Incremental replica fix and BOOX upgrade (2026-10-12)

Implemented ADR 0004: cached materialized models, incremental durable journal
transactions, a shared local command proxy, asynchronous Android persistence,
and cached settled-ink rendering. Numeric parsing no longer compiles a regex per
coordinate. The primary remains the existing desktop Python server and database;
no Phenom II deployment or server restart was needed.

Built both CLI targets and the Android APK through `nix-shell`. Verification:

- `python3 tests/cli_checks.py`: 11 tests passed.
- `python3 tests/proxy_checks.py`: 6 tests passed, including concurrent proxy
  actors, atomic rollback, offline recovery, large catch-up, strict numeric
  parsing, and truncated/corrupt journal handling.
- `python3 tests/cli_android.py`: passed actual Android child CLI/proxy calls,
  concurrent offline ink, actor undo/redo, export, server synchronization,
  conflict resolution and restart persistence.
- `python3 tests/integration.py`: all two-emulator checks passed, including
  live partial strokes, layers, erasure, offline recovery, browser rendering,
  pages, infinite canvas, and stylus/finger behavior.
- `python3 tests/performance.py`: passed on an isolated API 36 emulator at
  1860×2480 with host GPU rendering, 1,000 strokes and 50 pressure points each.

Final performance results (milliseconds, p95):

| Measurement | First 100, excluding warmup | Strokes 901–1000 |
| --- | ---: | ---: |
| Input handler | 0.97 | 0.74 |
| Dispatch including queue | 17.04 | 13.59 |
| Input to completed draw | 16.82 | 16.82 |
| Window frame | 16.74 | 21.28 |

The maximum measured frame was 33.38 ms. The run took 157.73 seconds, recorded
zero queued-save backlog and zero settled-ink cache rebuilds, and drained in
768.56 ms. Incremental state matched full replay. All 1,000 strokes survived
force-stop and fresh CLI inspection; reopening passed cached-versus-uncached
canvas comparison. The benchmark initially caught whole-cache rebuilds caused
by adding persistence metadata to strokes; cache comparison now uses visual ink
properties. Detailed metrics and logs are in `.test-artifacts/performance/`.

Installed the same tested APK on the physical USB BOOX with `adb install -r`.
A fresh stopped-app backup is `.test-artifacts/boox-upgrade/before-final.tar`.
All 87 pre-upgrade operations were preserved. The `notes` replica synchronized
to 191 committed operations with no pending uploads, against desktop server
identity `2220a9fd-fead-4f9a-a601-14f5cd7ca87e`. The tablet uses
`http://127.0.0.1:8765` through ADB USB reverse to desktop port 8765. Recreate that
tunnel after reconnecting USB; offline edits remain available without it.
The installed APK SHA-256 matched the tested artifact:
`2c495578488b0da237f3101ebc1d9cffb4ba6654bfbb6050d5e58855d22d7222`.
The app launched successfully and its collected process log had no fatal Java
exception. The desktop local proxy runs in tmux `2:3` using
`~/.local/state/notemaker-desktop`, while the original server remains in `2:1.2`.

Physical stylus-to-display latency and E Ink refresh still require manual use
of the updated BOOX build. Emulator results measure application response and
rendering, not physical display scanout. Journal compaction and a guarantee of
Android background execution are outside this change; see ADR 0004 for limits.
