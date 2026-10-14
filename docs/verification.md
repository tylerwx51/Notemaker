# Verification

## Tiered automated test framework — 2026-10-13

Added `tests/run.py` with independent local, emulator, hardware and network
tiers, streamed suite logs, JSON pass/fail/skip reports and resource-required
mode. Local is the default and neither discovers nor requires external devices.
The physical-tablet workflow is in `tests/tablet_checks.py`; installation and
USB port mapping are explicit options. `tests/network_checks.py` verifies
existing proxy trees using uniquely named test documents. See ADR 0008 and the
README's automated test tiers section for commands and resource requirements.

Executed through Nix:

```sh
nix-shell --run 'python3 tests/run.py'
nix-shell --run 'python3 tests/run.py --tier local --suite framework_checks --no-build'
nix-shell --run 'python3 tests/run.py --tier local --tier emulator --list'
nix-shell --run 'python3 tests/run.py --tier hardware --serial eac89823 --no-build'
```

The complete default local run passed its CLI build and all six suites. The
final framework-only run passed 11 checks, including additional simulated ADB
upgrade tests: backup precedes install, changed private data and a mismatched
APK fail verification, and failed installs relaunch the app. Other checks cover
skip/required-resource exit codes, failed subprocess reporting, missing/changed
actions, actor history, wrong root rejection before network edits, and the actual
network harness against an isolated two-proxy chain. The local topology suite
passed all eight checks including 25 generated histories. Python compilation
and `git diff --check` passed.

The selected physical tablet was available, but its smoke check **failed** with
zero pending actions and an upstream connection error. Read-only inspection
confirmed its USB mapping remained intact while no desktop server was listening
on port 8765. This was reported independently from the passing local tier.
The check did not reinstall the app, edit notes, or restart the external server.
The retained report is under
`.test-artifacts/runs/20261013-210329-405506/`. Failure screenshot/log collection
was subsequently added and exercised through the simulated-device failure tests.

No multi-computer lab was available for this run; the external-network harness
was verified on real local proxy processes. The existing emulator suites were
registered and their selection plan checked, without repeating the previously
recorded emulator runs. The new optional upgrade command was exercised with a
strict fake ADB; the equivalent physical upgrade performed earlier is recorded
below. Physical pen/E Ink performance remains outside these smoke checks.

## Durable proxy topology — 2026-10-13

Implemented protocol 3 durable proxy trees with one upstream per proxy,
exclusive replica ownership, proxy-only mutations, consistent direct reads,
offline provisional actions, root final ordering, durable acknowledgements,
SSE catch-up, and bounded/rate-limited uploads. See [ADR 0007](adr/0007-durable-proxy-trees.md).

Executed through the root Nix environment:

```sh
nix-shell --run 'apps/cli/build.sh --android && apps/android/build.sh'
nix-shell --run 'python3 tests/topology_properties.py'
nix-shell --run 'python3 tests/cli_checks.py'
nix-shell --run 'python3 tests/proxy_checks.py'
nix-shell --run 'python3 tests/server_checks.py'
nix-shell --run 'python3 tests/chat_checks.py'
nix-shell --run 'python3 tests/integration.py'
nix-shell --run 'python3 tests/cli_android.py'
nix-shell --run 'python3 tests/performance.py'
```

The JVM/DEX CLI and APK builds passed. The topology suite passed 25 generated
real-process histories plus targeted checks for cycles, exclusive ownership,
offline proxy chains, server identities, 500 ms first-action batching, upload
spacing/concurrency, bounded backlogs and idle behavior. The additional
existing-document wake-up regression passed separately. Generated histories
include up to five proxies, concurrent object/stroke/diagram actions, network
partitions, duplicate deliveries, lost responses, proxy crashes and main-server
restarts. Assertions check acknowledged-action retention, consistent atomic
diagram snapshots, unique final commits and model convergence under the main
server's final order.

CLI (16), proxy (6), server (3), and chat (7) checks passed, including actual
CLI processes, read-only direct access, actor history, SVG exports, legacy
replica migration and reuse of a surviving chat proxy after server restart.
Python source compilation and whitespace checks passed.

The complete two-emulator integration suite passed: simultaneous pressure-bearing
drawing, layer visibility, erasure and undo/redo, offline edits, app/server
restarts, browser/native image comparison, multipage and infinite documents,
live partial strokes before finger-up, and reopening an existing document.
The timed gesture observes the running proxy through an ADB-forwarded snapshot
endpoint, avoiding repeated ART startup during its seven-second window;
convergence checks still use the CLI's direct local reads. The harness also
wakes/unlocks the test devices and waits for the editor before gesture injection.

The full Android CLI suite passed app-spawned CLI/pen concurrency while offline,
actor-specific atomic undo/redo, SVG placement/export, desktop/tablet convergence,
app and server restarts, external selected-proxy writes by both app and CLI,
remote snapshot/SSE refresh, and creation/rendering of a multipage document
through the selected external proxy.

The 1,000-stroke emulator performance suite passed all absolute latency and
growth limits, incremental/replayed model equivalence, cached/uncached rendering,
and recovery of all 1,000 strokes with 50 points each after process restart.
Across the three measured windows, maximum p95 handler/dispatch/input-to-draw/
frame times were 1.36/17.24/16.83/21.41 ms. The save queue was empty at every
stroke-boundary sample; final drain took 773 ms and no ink-cache rebuild occurred.
Metrics and logs are in `.test-artifacts/performance/`. These are emulator
measurements, not physical stylus or E Ink latency results.

No physical BOOX installation, personal-document changes, live-server upgrade,
or home-server deployment was performed for this architecture change.
Protocol 3 requires upgrading the participating proxies and main server together.

### Physical BOOX proxy upgrade — 2026-10-13

Installed the verified protocol 3 APK on connected Note Air6 C `eac89823`
with `adb install -r`. Before installation, stopped the app and backed up its
installed APK, files and preferences in
`.test-artifacts/boox-proxy-upgrade-20261013-205148/`. All 17 data/preference
files were byte-identical immediately after installation. The installed APK
matched build SHA-256
`cc75f48e03951da36ef005f5bc052a6f462655b730d5e356f26544f09738c206`.

The configured desktop server was already running protocol 3; no server restart
was needed. Restored the missing `adb reverse tcp:8765 tcp:8765` mapping and
launched the app. The tablet proxy reported connected, zero pending actions,
no upstream error, and document cursor 411 at the verification checkpoint.
All 325 pre-upgrade action IDs remained present and actor histories were
unchanged. The screenshot showed the saved drawing and Connected status;
the running app log contained no fatal exception.

Read-only verification used the JVM CLI against the tablet proxy through a
temporary ADB forward, removed afterwards. A separate `run-as app_process`
CLI probe reached “Shutting down VM” but did not exit on this firmware;
its test process was terminated. Standalone on-device CLI process shutdown
therefore remains a hardware follow-up. This did not prevent the embedded
proxy or editor from synchronizing. Physical pen latency and E Ink refresh
quality were not measured.

## SVG placement — 2026-10-13

Executed through the root Nix environment:

```sh
nix-shell --run 'apps/cli/build.sh --android && apps/android/build.sh'
nix-shell --run 'python3 tests/cli_checks.py'
nix-shell --run 'python3 tests/proxy_checks.py'
nix-shell --run 'python3 tests/server_checks.py'
nix-shell --run 'python3 tests/cli_android.py'
```

JVM CLI, Android DEX CLI, and signed APK builds passed. The final CLI suite passed
14 checks, proxy suite 6, and server suite 3 (CLI/proxy suites also inherit the
server checks). Actual CLI processes exercised SVG file and raw-stdin import,
structured placement, page/layer targeting, intrinsic dimensions, prefixed XML,
source persistence, movement, actor-specific undo/redo, duplicate-ID rejection,
proxy file transfer, and synchronization to a second replica. Malformed XML,
invalid geometry, oversized source, scripts, external references, XML entities,
and processing instructions failed without partial edits. The valid SVG profile
was accepted by both Java and Python validators.

The isolated API 36 emulator check passed app-spawned SVG file import through
the local proxy while offline, undo/redo, desktop/tablet convergence, and app
restart persistence. A fixture with a nonzero viewBox origin, gradient, transformed
group, curved path, circle, and text rendered at the expected position. Chromium
pixel probes checked Android CLI PNG, JVM PNG, embedded SVG export, and the live
browser canvas. Artifacts are in `.test-artifacts/cli-android/`. Existing concurrent
pen input, object editor, history, and reconnect checks passed in the same run.

No remote deployment, personal-document edit, or physical BOOX installation was
performed. E Ink behavior and physical-tablet SVG rendering remain untested.
Font metrics and CSS details may vary across rendering engines; this is a static
SVG subset, documented in the CLI README and ADR 0005.

### Physical BOOX follow-up — 2026-10-13

Investigated the physical BOOX's reported desynchronization after placing the
generated SVG in the live `notes` document. Its configured URL was
`http://127.0.0.1:8765`, but `adb reverse --list` was empty. The installed APK was
also the October 12 build, before SVG support. At inspection, its cursor was 256
with 57 pending operations. Its pinned server identity matched the live server.

Stopped the app and backed up its files/preferences and installed APK under
`.test-artifacts/boox-svg-sync/`. Restored `adb -s eac89823 reverse tcp:8765 tcp:8765`.
An explicit sync using the old installed CLI then failed with `Unknown shape`,
confirming that connectivity alone could not resolve the backlog. Installed the
tested current APK using `adb install -r`, preserving app data, and reopened it.

The upgraded app reached cursor 418 with zero pending operations. All 301
operation IDs from the stopped-app backup, including the 57 previously pending
operations, were present in committed events; actor histories were unchanged.
The BOOX and server-synchronized desktop models matched exactly (56 strokes and
one SVG object at that checkpoint). The physical device screenshot showed
**Connected** and the mountain illustration rendered on the canvas. No fatal app
exception appeared in the collected logs. This verifies physical-device SVG
rendering and synchronization, but not stylus latency or E Ink refresh quality.
The USB tunnel still needs recreation after a USB/ADB connection reset.

## Earlier application verification

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

## Agent popup and sandboxed Pi chats (2026-10-13)

Built the JVM/Android CLI artifacts and Android APK through `nix-shell`.
Implemented ADR 0006 and verified:

- `python3 tests/server_checks.py`: 3 document protocol checks passed.
- `python3 tests/chat_checks.py`: 6 checks passed, covering incremental delivery
  before completion, reconnect cursors, duplicate/concurrent message submissions,
  Stop, real Pi cancellation during startup, global pagination, restart recovery,
  input validation, cross-origin rejection, and sandboxed CLI edit/sync.
- The sandbox test confirmed repository writes, upstream access, and denial of
  reads outside its mounts, including the user's SSH directory.
- `python3 tests/cli_checks.py`: 15 checks passed, including real CLI calls proving
  `NOTEMAKER_ACTOR` defaults and explicit actor overrides.
- `python3 tests/chat_ui.py`: passed on an isolated API 36 emulator. Verified five
  recent chats, More chats, mock/model/effort selection, replies, expandable tool
  output, reopen/catch-up, and draft persistence. Screenshots are under
  `.test-artifacts/chat-ui/`; no fatal Android exception was recorded.
- `python3 tests/pi_chat.py`: passed using installed Pi 1.2.0, openai-codex /
  GPT-6-luna at medium effort, real authentication, and Bubblewrap. Hooks emitted
  text deltas and tool events. Pi created and synchronized a test layer via the
  CLI under the exact chat actor. A second turn resumed the native session and
  correctly recalled the layer's name. All note edits used a temporary server,
  database, and replica.

Live testing caught stdout redirection/backpressure in Pi RPC mode; the hook now
uses a private per-run event file, tailed during execution and removed after
consumption. It also caught NixOS's indirect system certificate path; the harness
supplies its resolved certificate bundle to Node without disabling verification.
Actor identity is supplied through the environment to avoid model transcription
errors.

The primary server on port 8765 was not restarted and this APK was not installed
on the physical BOOX as part of these checks. Physical E Ink refresh, keyboard
behavior, and stylus interaction while the popup is open remain untested. No
Phenom II deployment was performed. Offline chat-history browsing is not
implemented; drafts remain local and history is fetched from the server.

### Physical BOOX agent upgrade (2026-10-13)

At the user's request, installed the tested APK on BOOX Note Air6 C serial
`eac89823` using `adb install -r`. Before installation the app was stopped and
its files/preferences archived to
`.test-artifacts/boox-agent-upgrade-20261013-103145/before.tar`. All 17 files were
byte-for-byte identical immediately after installation. The installed APK was
pulled back and its SHA-256 compared with the build artifact.

Backed up the live server database using SQLite's backup API, then restarted the
existing local server in tmux pane `2:1.2` through the updated `nix-shell` on port
8765. Its identity remained `2220a9fd-fead-4f9a-a601-14f5cd7ca87e`, and all 363
existing document events were unchanged. The live chat options endpoint returns
Pi / openai-codex / GPT-6-luna / medium. Restored the tablet's USB reverse tunnel
for port 8765.

The app launched successfully and the Agent button was found. Full popup
inspection was interrupted when the foreground switched to BOOX Settings; no
chat was submitted. The app process log contained no fatal Java exception.
Verification details and the installed APK copy are in the same backup directory.
Physical refresh and keyboard behavior still require manual use.

### Pi provider changed to openai (2026-10-13)

Changed both default Luna model options to provider `openai`, retaining
GPT-6-luna / medium as the default. Updated selector expectations, configuration
examples, and ADR 0006. Six chat checks and the live Pi test passed using the
host's current `openai` credentials; the live test verified streamed text/tool
hooks, CLI edits under the chat actor, synchronization, and session continuation.

Backed up the live database and private credential copy under
`.test-artifacts/openai-provider-20261013-105110/`, changed the provider of all
three existing Pi chats from `openai-codex` to `openai`, refreshed the dedicated
credentials from the host Pi configuration, and restarted the existing server
on port 8765. Verified the live options and existing-chat endpoints return
`openai`. Server identity, document events, and chat history were preserved.
The tablet loads provider options from the server; no APK change was needed.

### Chat keyboard sizing and readable tools (2026-10-13)

Changed the popup to a transparent full-window host with a centered panel sized
from the available area after system and keyboard insets. The transcript gives
up height while the composer and Send/Stop controls remain visible. Hiding the
keyboard restores the original panel size. The implementation follows Android's
[keyboard inset guidance](https://developer.android.com/develop/ui/views/layout/sw-keyboard)
and retains adjust-resize behavior on Android versions before API 30.

Tool headings now include the filename, shell command, or search pattern.
Expanded arguments show readable labels, preserved command text, edit before/after
text, and a separate Output section. Unknown tools get labeled arguments, and
malformed/truncated arguments fall back to their original text.

Built the APK inside `nix-shell`. The updated `tests/chat_ui.py` passed on an
isolated API 36 emulator, including keyboard-visible input and Send bounds,
sending with the keyboard open, restored size, readable read/bash/edit fixtures,
and the existing chat/history/draft checks. Screenshots include
`.test-artifacts/chat-ui/keyboard-visible.png` and `tool-formatting.png`.
The final run completed without a fatal exception in the NoteMaker process.

Installed on BOOX `eac89823` with `adb install -r`. All 17 app data/preference
files were identical before and after installation; backup and verification
artifacts are in `.test-artifacts/boox-chat-layout-20261013-110736/`.
The installed APK matched the build SHA-256:
`6e113236378821cee61817ae1ced25c5d35e6c2a041bab9f9ad58fccd464831c`.

Woke the BOOX from its dozing/screensaver state and checked an existing chat
without typing or sending a message. With its keyboard visible, the keyboard
started at y=1725, the message field ended at y=1589, and Send ended at y=1679.
Hiding the keyboard restored Send to its original y=2275 position. Screenshots
and `layout-check.json` record these physical-device measurements. The local
server required no restart for these Android-only UI changes.
