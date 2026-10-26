# Verification

## Pre-commit emulator runtime — 2026-10-26

Ran `.githooks/pre-commit` through the Nix development shell with six workers
and five emulator slots. The full hook passed **22/22 checks in 292.23 seconds**
by an external monotonic timer; the runner reported 289.08 seconds after its
startup. The seven emulator suites took: performance 90.04 s, integration
182.92 s, CLI Android 112.54 s, drawing Android 137.73 s, viewport Android
130.04 s, files Android 75.61 s, and chat UI 60.91 s. These suites overlap
except for performance, which ran first and alone. Its 1,000-stroke check had
zero events over 30 ms and a 21.79 ms maximum. The run report is
`.test-artifacts/runs/20261026-034559-272573/report.json`; the hook transcript
is `.test-artifacts/phase-profile/full-six-workers-hook.log`.

The ordinary x86_64 API 36 emulator was used. E Ink refresh, BOOX-specific
behavior, physical stylus timing, and the external server/network tiers were
not tested by this pre-commit run.

## GIF controls on the canvas — 2026-10-24

Built and installed the updated APK on the physical Note Air6 C. The fixed
**GIF Player Review** note now draws a previous/play/next row and a timeline
immediately beneath each of its three GIFs. Moved the captions and instruction
line in one atomic CLI action and confirmed the tablet replica received them.
The final tablet capture is `.test-artifacts/gif-review/inline-controls-final.png`.

The live-session CLI selected `tool.select` with `{"name":"interact"}` and
`tests/gif_player_android.py` passed its control checks. Actual tablet taps on
the first animation toggled play and pause, advanced one frame, and moved back
one frame. Dragging its timeline selected frame 5. A tap on the second
animation played it while the first remained paused at its own frame. Both
returned to frame 0, paused; the note's revision stayed 27 with zero pending
document actions. The note is left open with Interact selected. Perceived E Ink
refresh quality still awaits user feedback.

## GIF player — 2026-10-24

Built the JVM CLI and Android APK in `nix-shell`; Python sources compiled.
`tests/server_checks.py` passed 7 checks, including a valid two-frame GIF and
rejection of a truncated GIF without a partial commit. `tests/cli_checks.py`
passed 25 checks. A fresh local proxy accepted a real `gif add` CLI call and
an atomic diagram with three GIFs and captions; PNG export showed frame zero.

Restarted the existing desktop primary server and desktop proxy with the GIF
schema, then created and synced the fixed **GIF Player Review** document.
Inspection found 3 GIF objects and 5 text objects on its new layer. Installed
the updated APK on the USB-connected Note Air6 C (`eac89823`), and the tablet
proxy downloaded the note with zero pending actions. A tablet screenshot
confirmed the captions and first frames were legible.

`tests/gif_player_android.py` passed against the physical Activity: action
discovery, paused start, next/previous, seek, timed advance, pause stability,
and unchanged document revision. A second Activity kept frame 2 paused while
the first played and sought independently. Tablet screenshots at frames 0 and
3 differed by 6,422 pixels within the moving-dot region. The player dialog
showed its timeline and three controls at readable sizes. A live
`gif.import.open` CLI action opened Android's Documents picker, which was
confirmed as the resumed Activity and then canceled. At that stage the note was
left open at frame 0, paused, with its original player dialog visible. The
tablet's original LAN server
address was restored after USB synchronization; its Wi-Fi was disabled during
the check, and the note remains in its local replica. Perceived E Ink refresh
quality and comfortable playback speed await user feedback.

## App session ID in agent chat — 2026-10-21

The Android chat send now includes the current app launch session ID. The server
journals it with the user message and supplies it to Pi as `app_session_id` in
the routing context. Legacy requests without a session remain accepted.
The APK built in the root Nix shell, all 9 `tests/chat_checks.py` checks passed,
and `tests/chat_ui.py` passed on an API 36 emulator. That Android check compared
the stored chat request's session ID to the live server session registry.

Backed up the live database and agent data under
`.test-artifacts/chat-session-deploy-20261021/`, then restarted the existing
`notemaker-primary` service. Installed the APK on physical BOOX `eac89823`
with the backed-up `tests/tablet_checks.py --install` workflow. The check
preserved 64 app files/preferences and 266 existing actions, confirmed zero
pending edits and a connected proxy, and matched installed APK SHA-256
`e94cfee9be55ff1bc0d1bb84c8d6d70e494b6676a4b70f92ec879187544b5779`.
Tablet artifacts: `.test-artifacts/tablet/20261021-132001-341831/`.
The desktop CLI listed the restarted tablet's session
`2a27808d-5055-4b1d-bfdc-f0d9ddb1c8fb` with `abc` open. The live Pi provider
was not invoked for this change; the server-side prompt transformation was
checked through code review and the API/Android integration checks.

## Networked live app sessions — 2026-10-21

Built the JVM/Android CLI and APK in the root Nix shell. Passed
`tests/ui_sessions_android.py` and `tests/ui_topology_android.py` with real API
36 emulator processes and actual CLI calls. The topology check ran a main
server plus two proxy hops, discovered the Android session with device and
last-seen metadata at the root, changed the app's open document to `abc` from a
different proxy, and observed the session expire after the app closed.
Passed the server (6), proxy (17), and CLI (25) regression checks and
`git diff --check`.

Backed up the live desktop server database and proxy configuration to
`.test-artifacts/ui-network-deploy-20261021/`, then restarted the existing
`notemaker-primary` and `notemaker-desktop` tmux services with the updated
server and CLI. The main server served `/api/ui/sessions`, its document list
still contained `abc`, and the desktop CLI returned
`{"sessions":[],"scope":"root"}` through its authenticated proxy. There
were no live sessions at the time of that first check.

### Physical Note Air6 C deployment

Later, the tablet appeared as ADB serial `eac89823`. Installed the new APK with
`tests/tablet_checks.py --serial eac89823 --install --timeout 90` in the Nix
shell. The backed-up upgrade check passed: 64 app files/preferences and both
existing document actions were preserved, the proxy reconnected with zero
pending edits, and the installed APK SHA-256 was
`1b25f4bc19fd923448c3436497ba48975f421c9b22a0b15e0f57b73873dcee3c`.
Artifacts are in `.test-artifacts/tablet/20261021-131122-843023/`.

The main server and desktop CLI both listed session
`c3d3f3ed-fab9-43ad-9ce8-6c1c420f21c6` on `ONYX NoteAir6C`, including its
device ID, proxy ID, open document, foreground flag, and last-seen/expiry
times. Through the desktop proxy, `ui action` opened `abc` on the physical
tablet; a separate `ui state` call confirmed `document: abc`. BOOX E Ink
refresh and stylus behavior were outside this control check.

## Live app sessions and CLI controls — 2026-10-21

Built the JVM/Android CLI and signed APK inside the root Nix shell. The focused
`tests/ui_sessions_android.py` check uses actual CLI processes through an ADB
forward to the tablet's authenticated local proxy. It checks session discovery,
the action registry, Files browser state, document and page navigation, tool and
paint controls, background/foreground continuity, and a fresh session ID after
app restart. The regular CLI suite was also run for regression coverage.
BOOX E Ink refresh and stylus behavior remain untested.

### Physical tablet deployment — 2026-10-21

Installed the session-control APK on the USB-connected Note Air6 C
(`eac89823`) using `tests/tablet_checks.py --install --timeout 90` inside the
Nix shell. The backed-up upgrade check passed: 64 private files/preferences
were preserved, the installed APK matched SHA-256
`a65766d9de9b12ba52e4592259ebdd5613665491a1851dea0f93c9a99116b5d7`,
and the two existing actions in the open `Matplotlib chart grid` note remained
intact. Its embedded proxy reconnected with zero pending edits. Backups and the
report are in `.test-artifacts/tablet/20261021-125651-740809/`.

Through an ephemeral ADB forward, the JVM CLI discovered the live session and
all 35 registered UI actions. A `panel.open` command opened Notes on the tablet,
and `panel.close` closed it; the open document remained `Matplotlib chart grid`.
The USB forward was removed afterward. No home-server deployment was needed.

## File browser — 2026-10-14

The grid/list follow-up also passes the APK build and isolated offline browser
suite. The default grid renders actual saved ink; instrumentation verifies that
rendering leaves the document revision unchanged. UI checks cover switching to
List, retaining that choice after an app restart, returning to Grid, and the
existing creation/search/open flows. Reviewed `files-grid.png` visually; grid
and list screenshots are retained in `.test-artifacts/drawing-android/`.

Passed through the root Nix shell: Android APK build, 18 CLI checks, 13 test
framework checks, and `tests/files_android.py` on an isolated API 36 emulator.
The offline UI check creates a multipage file, rejects empty and duplicate
names, filters the file list, switches documents, restarts the app, and verifies
the saved document type. Screenshot and logs are in
`.test-artifacts/drawing-android/files-*`. The suite is included in the emulator
tier. The README and ADR 0012 describe replica scope and existing CLI commands.
Installed on BOOX `eac89823` using the backed-up upgrade workflow. All 52
data/preferences files matched immediately after installation, and all 131
actions and actor histories in the active document were preserved after launch.
The proxy reconnected with zero pending actions. Installed APK SHA-256:
`0fab2a801425ea46bf3952ea4304d411aa41915425e9b807618f49acba269a94`.
Backups and smoke results for the grid/list update:
`.test-artifacts/tablet/20261014-205522-807085/`.
E Ink behavior and remote proxy browser UI were not checked.

## Erase latency optimizations — 2026-10-14

Implemented the follow-up on `perf/svg-display-cache`, preserving the baseline
and high-memory experiments on separate branches. The [report and matplotlib
charts](design/latency/report.md) include sanitized measurements, methodology,
branch references, cache memory limits and the remaining latency gap.

Final physical BOOX partial-erase feedback measured median 21.2 ms, p95 30.1 ms,
27/29 under 30 ms. A prior run measured median 15.7 ms, p95 32.0 ms. Whole-item
feedback in the verified repeat measured median 54.9 ms, p95 86.2 ms. Durable
updated draws remained around 85–102 ms median. These are app draw timings on
the reported 40 Hz display, not physical E Ink scanout. The offline isolated
package restored source geometry/visibility after every trial and was removed.

Passed through Nix: CLI (18), coverage (11), drawing tools (9), selection (20),
proxy (9), conflicts (6), browser rendering/export, Android coverage and panels.
Coverage instrumentation additionally verifies exact cache/full-render pixels
for nine edit states, fractional SVG Picture replay, and pending feedback
rollback when a foreign lease arrives before persistence. Physical original/new
coverage signatures match over 8,700 queries. Captures and detailed instrumentation
remain in `.test-artifacts/latency`; published artifacts contain no note content.

The 1,000-stroke performance regression passed on API 36: input-to-draw p95
16.79–16.88 ms across three windows, maximum save queue 1, final drain 732 ms,
zero full ink-cache rebuilds, replay equivalence, cached/uncached pixel matching
and durable restart. Artifacts: `.test-artifacts/performance/`.

Installed the verified build on BOOX `eac89823` using the backed-up upgrade
workflow. All 49 data/preferences files matched immediately after installation;
all 358 actions and actor histories were preserved after launch. The configured
proxy connected with zero pending actions. APK SHA-256:
`f45a248e17625ea939865071c5acf55a28d8003995f61dee04b878bb823c7dbc`.
Backup and smoke results: `.test-artifacts/tablet/20261014-200826-752953/`.

## Erase latency investigation — 2026-10-14

Captured the physical NoteAir6C's app files read-only and profiled a disposable
copy of `Drawing object protocol.` using the installed APK's Java core and native
Android coverage renderer. The selected base layer/page contained 36 active items
and 3,296 stroke points. Neither the installed APK nor the original note was
modified. Temporary device replicas were removed after testing.

Added `tests/LatencyProbe.java` and `tests/drawing_latency.py`; the latter compiles
and runs the probe through the Nix toolchain, supports JVM and physical Android
execution, and requires an explicit capture and a fresh output directory.
All writes go through shared Store/LocalProxy actions in the disposable replica.
The marker check prevents accidentally targeting an ordinary replica directory.

Final tablet run:

```sh
nix-shell --run 'python3 tests/drawing_latency.py --archive .test-artifacts/latency/tablet-files.tar --document "Drawing object protocol." --output .test-artifacts/latency/tablet-final-profile --serial eac89823'
```

| Phase | Measured time |
| --- | --- |
| Cold state read/replay | 150 ms |
| Warm state read | 0.15–0.25 ms |
| Single-region hit tests across selected layer | 1,274 ms |
| Coverage factory portion of those hit tests | 1,271 ms (36 calls) |
| Repeated erase geometry | 1,246–1,249 ms |
| Complete local proxy erase command | 1,253–1,300 ms |
| Snapshot following erase | 1.17–1.27 ms |
| Snapshot to model | about 0.47 ms |
| Undo | about 4 ms |
| 120-point swipe command | 1,264 ms |
| Full page PNG rendering and encoding | 94 ms |
| Fifth erase in an immediately submitted five-request burst | 6,801 ms |
| Experimental conservative ink bounds filter, cold coverage cache | 1.76 ms |

The burst intentionally uses the same region: the first erase changes content,
and the later requests report no matching content, but still take about 1.37
seconds apiece. Request four completes after 5.43 seconds and request five waits
5.43 seconds before starting. The single-thread executor mirrors the app writer
queue, but this is a controlled backend benchmark, not recorded touchscreen input.

Root cause: `DrawItem.intersects` constructs exact coverage before its bounds
rejection. `DrawingActions.erase` calls this for all items in the scoped layer.
Android `ItemCoverage` unions the painted segment/dot paths repeatedly when
building stroke coverage. The 16-item LRU churns while scanning this layer of 36
items. Coverage generation accounts for over 99% of the measured hit-test time.
The work occurs under Store's transaction/coordination locks, and the Android
single write queue serializes subsequent erases and other writes behind it.
This explains how a local geometry cost becomes a multi-second backlog without
any server dependency.

The test-only bounds experiment clears the coverage cache, conservatively
rejects distant ink before calling exact coverage, and reduces candidates to
nine while retaining the same two hits. It is evidence for the optimization
direction, not a production fix or a general latency guarantee. Production
coverage, caching and UI behavior remain unchanged in this investigation.

The JVM baseline also reproduced the geometry bottleneck (about 1.4 seconds per
erase, sub-millisecond snapshots). Physical runs were offline with no upstream;
connected-server timing, Activity frame presentation and E Ink panel latency
were not measured. The PNG figure includes rendering and encoding and must not
be interpreted as touch-to-display latency. Earlier functional emulator tests
do not cover this real-note erase workload.

Artifacts: `.test-artifacts/latency/tablet-final-profile/timings.jsonl`, with
earlier repeat runs under `.test-artifacts/latency/`. The capture contains private
note data and is not committed. Python syntax and `git diff --check` passed.

## Unified item coverage and layer-scoped editing — 2026-10-14

Built JVM CLI, Android CLI and APK through `nix-shell` using
`apps/cli/build.sh --android` and `apps/android/build.sh`.

The local regression run passed server, CLI, drawing, drawing-tools, coverage,
browser, selection, conflict and proxy suites (87 cases at that point). Report:
`.test-artifacts/runs/20261014-154041-029503/report.json`.
Focused reruns passed ten coverage cases, nine drawing-tools cases and twenty
selection cases through actual CLI calls. The latest coverage run also verifies
that masks on appended ink do not reuse old action metadata. Coverage
includes alpha holes/opaque white pixels, hollow shapes, pressure ink, glyphs,
layer-scoped direct IDs, atomic masks/undo/redo, transforms, restart persistence,
flood-fill gaps, empty coverage and retaining both offline mask-conflict versions.
The browser test also checks masked PNG export and pixels rendered after syncing
an erased image through a temporary local Python server.

The focused Android coverage check passed on an isolated API 36 emulator,
including image alpha, native outlines, partial-erasure pixels revealing the
underlying layer, and releasing selection leases on a layer switch. The broader
Android run passed fill, selection/preview/transform/delete, foreign leases,
box/lasso selection, layer compositing, conflict comparison and persistent
clipboard checks. Its later panel-navigation test exposed outdated menu labels;
the test now uses the existing Template, Layers and More controls. It also exposed
lightweight stroke previews lacking a `kind` field; the shared mask reader now
handles these, and layer thumbnails retain brush and erasure data.
The final `python3 -u tests/drawing_android.py --coverage-only --panels-only`
run passed both groups on the rebuilt APK: coverage/layer-switch checks and brush,
eraser, template, layer-action and toolbar-preference panels. Artifacts are in
`.test-artifacts/drawing-android/`, including the final `logcat.txt`.

Installed the rebuilt APK on BOOX NoteAir6C `eac89823` through
`nix-shell --run 'python3 tests/tablet_checks.py --serial eac89823 --install'`.
The backup retained the previous APK and app data; all 49 app-data/preferences
files were byte-identical across installation. The installed APK matched SHA256
`04e5632f5e8ac18f5a68bf760d017e22b20a22890ce324842db8a979377a96a8`.
The connected smoke check failed because the configured upstream
`127.0.0.1:8765` was unreachable, with zero pending actions reported. A subsequent
read-only proxy history query timed out, so post-launch history comparison was
not completed. Bringing MainActivity to the foreground succeeded; screenshot
inspection confirmed the editor displays the existing drawing. AndroidRuntime
logs collected during startup contained no fatal exception.
Artifacts: `.test-artifacts/tablet/20261014-185835-448778/`.

No E Ink/stylus validation or remote server rollout was performed.
Raster coverage uses a one-local-unit grid; font/SVG edge behavior
follows the platform renderer. See ADR 0011 for memory bounds and upgrade scope.

## Agent robot toolbar shortcut — 2026-10-14

Added an original monochrome vector robot above More in the left rail. It opens
the existing Agent interface directly; the More shortcut uses the same icon.
Built with `nix-shell --run 'apps/android/build.sh'` and installed through the
backed-up tablet check on BOOX `eac89823`. All 43 app-data files and 544 existing
actions were preserved. Sync connected with zero pending actions, and no fatal
exceptions were found. Android UI automation tapped Open Agent, verified its
Close control, and returned to the editor. The toolbar screenshot was inspected.
Artifacts: `.test-artifacts/tablet/20261014-115346-194797/`.
APK: `5b47aed9a8c12bf3eb9e24a9b6ec96a0308a46700abe49e2a08f584176c512b4`.

## BOOX-style editor organization and menus — 2026-10-14

Reorganized the Android editor into a narrow left rail, top-bar drawing/history
controls and anchored tool panels. Layers now opens a thumbnail list instead of
covering the canvas with a permanent tray. White panels use black outlines,
divided action rows, icon grids and selection dots. See ADR 0010.

Passed through the Nix development environment:

- `apps/android/build.sh` (the existing Java 8 bootstrap/deprecation warnings remain).
- `python3 tests/ui_review.py emulator-5554`: real UI clicks, pen color, layer
  creation/visibility/force rules, undo/redo, rectangle placement as one atomic
  batch, panel touch isolation, unchanged canvas bounds, label preference after
  restart and narrow-screen scrolling. Screenshots in `.test-artifacts/ui-review/`.
- `python3 tests/drawing_android.py --tool-switch-only`: existing selection leases,
  delayed acquisition/reselection and transform-preview cancellation regression
  passed on an isolated emulator.
- Python syntax validation of the updated UI review and `git diff --check`.

A focused final-APK UI check also passed document-menu access and scrolling the
narrow left rail to More; its screenshots are `documents-popup.png` and
`narrow-more-popup.png` in the same artifact directory.

The first UI run caught a test assertion that expected a standalone object event;
the rectangle was correctly stored in an atomic batch. The review now asserts
exactly one rectangle batch. Screenshot inspection also caught panel contents
covering the rounded border; the shell now draws its outline after its contents.
No document action or shared protocol was added or changed.

Installed APK `51726b2291078de45d5712aab23584763670fe0cfd9e877638bb63676a424c18`
on BOOX `eac89823` with `python3 tests/tablet_checks.py --serial eac89823 --install`
through Nix. All 43 app-data files were byte-identical across installation, and
the installed APK matched the build. The connected smoke check timed out because
the configured upstream was unavailable. A separate read-only CLI check against
the running tablet proxy verified all 484 existing actions and actor histories,
including the same five pending actions before and after installation. The app
opened with the new rail, with no fatal exceptions in its process log; its screen
was captured and inspected. Artifacts, backup and offline verification report:
`.test-artifacts/tablet/20261014-113915-173380/`. E Ink refresh and physical stylus
ergonomics remain unverified for this build.

## Selection ends on tool change — 2026-10-14

Tool changes now clear the selected entities, stop lease heartbeats, cancel
uncommitted previews and release locks through the selected proxy. Selection
generation checks discard and release delayed acquisitions, including a pending
per-item deselection that would otherwise reacquire the remaining entities.
Old preview callbacks cannot restore a canceled preview. Reopening Select keeps
the active selection.

Built with `nix-shell --run 'apps/android/build.sh'`. The focused isolated-emulator
regression passed with
`nix-shell --run 'python3 tests/drawing_android.py --tool-switch-only'`.
It checks Pen, Eraser, Move, Shapes, Text, Fill and direct shape placement;
selection acquisition and per-item reselection races; lease removal; and
preview cancellation without changing drawing state or undo history. The same
regression is included in the normal Android drawing suite.

Installed APK
`03e8e05f9a24f520692306f1d6642fac75fceac116762b6d62b4f8f1f85ddd1c`
on BOOX `eac89823` with the backed-up installation check. All 40 app-data files
were byte-identical across installation, all 479 existing notebook actions were
preserved, and the proxy connected with zero pending actions. Installation
artifacts: `.test-artifacts/tablet/20261014-095651-503791/`.
The same tool-switch regression passed on the BOOX in an isolated test notebook
with the upstream unavailable. The original `notes` selection, upstream, root
identity and all 479 original actions/actor histories were preserved. Results:
`.test-artifacts/selection-tool-switch/20261014-095432/`. The temporary desktop
server used for the installation smoke check was stopped afterward.

## Full drawing and collaboration feature set — 2026-10-14

Implemented the remaining ADR 0009 groups with GPT-6-luna implementation
agents: brush profiles/presets, geometric and 3D figures, text styling,
selection and atomic transforms, copy/cut/paste, erasers and enclosed fills,
layers, embedded pictures, background templates and grouped tool panels.
All durable edits use the selected proxy and local replica. Selection leases
and live previews are transient; causal conflict snapshots and their resolution
are replicated and undoable. Handwriting recognition remains deferred.

The integrated local run at
`.test-artifacts/runs/20261014-011936-401576/report.json` passed framework,
server, CLI, drawing, drawing-tools, Chromium rendering, selection, conflict,
proxy and chat suites (97 test executions, including inherited shared checks).
Its topology suite exposed two test-fixture assumptions: every POST had been
counted as a saved action upload. Fixtures now restrict upload counters and
transport fault injection to `/api/events`, allowing `/api/locks` coordination
traffic through independently. The CLI lost-ack fixture received the same fix.
The corrected topology suite passed all 10 tests, including its generated
network examples. The focused clipboard test passed after adding copy timestamps
to the proxy's local clipboard metadata.

A complete local rerun passed all 108 test executions across all eleven
suites: `.test-artifacts/runs/20261014-021929-135803/report.json`. It includes the
new regression requiring a committed transform to reach the upstream server
without an explicit sync. Transform commits, selection mutations and conflict
resolutions now schedule uploads; transient previews and leases remain outside
the saved action history.

The broad emulator regression caught a pen-tail bug: with scribble detection
disabled, ACTION_UP failed to emit the last stroke snapshot. Short gestures
could persist only their first point. Final stroke emission is now unconditional
unless the gesture is consumed as a scribble erasure; instrumentation asserts
that all 12 injected points reach the saved stroke.

The existing Android CLI and chat UI suites passed in
`.test-artifacts/runs/20261014-012822-568797/`. The initial performance preflight
ran before the preceding chat emulator disappeared from ADB; its later run
passed in `.test-artifacts/runs/20261014-014156-350899/`. It verified 1,000 durable
strokes, incremental/full replay and cached/uncached image equivalence, queue
drain below one second, and the existing latency/growth budgets. The last
100-stroke window had 1.27 ms p95 input-handler time and 16.74 ms p95
input-to-draw time. These are emulator measurements, not E Ink latency claims.

The longer integration run exposed live pen uploads waiting behind a fetch-only
sweep of inactive notebooks. Captured Java stacks showed the sync worker waiting
on replica and lock requests while the active notebook accumulated pending ink.
The scheduler now prioritizes the active notebook and pending edits, preempts
stale fetch requests asynchronously, and retries intentional preemption without
disconnect errors or exponential backoff. Replica/info/lock requests have a
two-second inactivity timeout because HTTP disconnect alone did not reliably
interrupt a blocked JVM read. Empty lock-update POSTs are skipped. The existing
500 ms upload gate remains in force.

A regression inserts an uncooperative four-second response delay for an idle
notebook, requires a new active edit to arrive upstream within three seconds,
and then verifies the idle notebook catches up. The integration harness also
waits for both proxies' upstream handshakes before timing delivery, retains its
original seven-second live-stroke budget, and records both source and destination
state on failure. Temporary Java-stack diagnostics were removed from production
status responses.

The final build passed all 109 local test executions across all eleven suites,
including the new stalled-background-fetch regression and all ten proxy-tree
properties: `.test-artifacts/runs/20261014-024548-026249/report.json`.
The complete two-emulator `tests/integration.py` run also passed on that build,
including live partial-stroke delivery before finger-up, simultaneous drawing,
undo/redo, visibility, offline/restart convergence, browser replay, multiple
document types and finger/stylus input. Its final output is retained at
`.test-artifacts/tablet-final/20261014-024511/integration-results.txt`.

The full expanded `tests/drawing_android.py` suite passed with APK
`d4219dc2cf22c62c72572db1b05daa66cfa63f2218409ff707c207649110b14f`.
It verifies foreign previews and cancellation through proxy notifications
without a revision change, rejected undo preserving model/history, unlocked
undo succeeding, box and closed freeform lasso selection, layer compositing
after reordering, templates behind ink, persisted conflict badges after restart,
side-by-side comparison and resolution/undo, clipboard across restart and
notebooks, and brush/eraser/template/layer/preferences panels. Screenshots and
instrumentation results are in `.test-artifacts/drawing-android/`.

Those tests caught and fixed Android-specific issues: startup loaded raw
replica data without coordination/conflict metadata; transient notifications
were ignored when the durable revision stayed unchanged; failed undo advanced
the UI history; layer order differed from export order; and closed freeform
lassos were mistaken for taps. Popup contents now scroll when needed, and the
comparison panel keeps both previews and all three resolution choices visible.

Installed the final APK
`76230903dc925dcc42328c92e68e1b2e90753f5187448e8e01d6574be53e1309`
on BOOX NoteAir6C `eac89823`. The backed-up upgrade preserved all 37 private
app-data files byte-for-byte and all 448 actions in the current notebook.
Installation, connectivity, APK comparison and history checks passed in
`.test-artifacts/tablet/20261014-024538-391481/`.

Physical BOOX checks on that build passed in
`.test-artifacts/tablet-drawing/20261014-024619/`: actual toolbar/text input,
shape creation/undo/cancel, ephemeral previews, atomic text scaling, clipboard,
fill, background template, partial erasing, complete pressure-bearing stroke
samples and offline restart. The original notebook selection, upstream, root
identity and all 448 original actions/actor histories were preserved. The test
notebook synchronized all 15 actions with zero pending actions through USB;
tablet and server action logs matched. The desktop database backup and sync
report are in `.test-artifacts/tablet-final/20261014-024511/`. Only the temporary
local server was stopped afterward; no home-server deployment was performed.
Hardware checks used ADB and instrumentation, including pressure-bearing input
samples. Physical stylus feel, tilt and E Ink ghosting still need hands-on
assessment; screenshot and software timing checks do not establish those.

Coverage uses actual CLI calls and proxy trees for competing selections,
15-second expiry, preview cancellation, clean owner shutdown, offline edits,
actor-specific undo, locked-operation rejection, and conflict convergence.
Conflict tests preserve successive versions on both offline branches, including
same-actor edits and hide-versus-edit conflicts; resolution and its undo/redo
propagate. Browser checks verify filled 3D figures, styled text and pictures,
rotated connector boundaries, and previews without durable event changes.
Partial erasing, clipboard diagrams, layers and templates are atomic and
reversible. Complex flood-fill work is bounded and fails without an action.

Commands used for the final verification:

```sh
nix-shell --run 'python3 tests/run.py --tier local --no-build'
nix-shell --run 'python3 tests/topology_properties.py'
nix-shell --run 'python3 tests/drawing_android.py'
nix-shell --run 'python3 tests/integration.py'
nix-shell --run 'python3 tests/run.py --tier emulator --suite integration --suite cli_android --suite chat_ui --suite performance --no-build'
nix-shell --run 'python3 tests/tablet_checks.py --serial eac89823 --install --reverse-port 8765 --timeout 90'
nix-shell --run 'python3 tests/tablet_drawing.py --serial eac89823'
```

## Physical BOOX drawing checks — 2026-10-14

Installed the current chunk-1 APK on BOOX NoteAir6C `eac89823` (Android 16)
after backing up its installed APK and private app data. Installation preserved
all 19 existing files byte-for-byte; the installed APK hash matched the build.
Compared the pre-upgrade replica with the running tablet and verified all 411
original actions, the main-server identity, and actor histories were retained.

Executed through Nix:

```sh
nix-shell --run 'python3 tests/run.py --tier hardware --serial eac89823 --install-tablet --require-resources'
nix-shell --run 'python3 tests/tablet_drawing.py --serial eac89823'
nix-shell --run 'python3 tests/tablet_checks.py --serial eac89823 --timeout 60'
nix-shell --run 'python3 tests/run.py --tier local --suite framework_checks --no-build'
```

The initial upgrade smoke check installed successfully but failed on a stale
proxy port. Instrumentation restarts on BOOX can change that port; tests now
refresh forwarding and wait for a successful read-only probe. Mutations are
not retried automatically. Early drawing attempts passed toolbar shape/text
creation but hit the same restart race; those failed reports are retained.

The first offline harness also cleared the upstream temporarily. Restoring the
preference alone did not restore the effective proxy edge when the existing USB
mapping led to an unavailable desktop server. The original edge and root identity
were restored through the proxy command pathway, briefly removing and then
restoring that exact USB mapping so the offline main-server address could be
accepted. No replica files were edited directly. The final harness preserves
the upstream throughout, requires it to be offline before starting, and checks
both preferences and effective proxy configuration afterward.

The final drawing run passed real UI taps/drag creation, text entry/placement,
star creation, atomic undo/redo, cancellation, a JVM CLI edit through the tablet
proxy, live app visibility of that edit, and offline process restart. Existing
notebook content and actor histories remained unchanged. The original `notes`
selection, host/proxy preferences, effective upstream, root identity and USB
mapping were preserved. Unique test notebooks are retained.

Then verified the desktop database had the tablet's pinned identity, backed it
up through SQLite's backup API, and started the local server temporarily on
127.0.0.1:8765. The final connectivity smoke check passed. All seven actions in
the final test notebook synchronized over USB; tablet/server action logs matched
and pending count reached zero. The original 411 actions and histories were
verified again. Only the temporary server process was stopped afterward.

Artifacts:

- `.test-artifacts/tablet/20261014-002028-995317/`: pre-upgrade APK/data,
  desktop database backup, installed APK verification, history preservation and
  connection-restoration reports; initial smoke failure is retained.
- `.test-artifacts/tablet-drawing/20261014-002905/`: final passing drawing
  report (including effective upstream preservation), screenshots and sync report.
- `.test-artifacts/tablet/20261014-003057-381651/`: passing connectivity smoke.
- `.test-artifacts/runs/20261014-002306-548371/`: 13 passing local framework checks.

Device screenshots confirm layout and rendered content, not physical E Ink
ghosting or pen feel. Input was injected through ADB/instrumentation; actual
stylus pressure, latency and refresh appearance still need hands-on review.
No remote home-server deployment was performed. The updated APK remains installed.

## Drawing tools, chunk 1 — 2026-10-13

Added Android shape/text creation through the selected proxy, original vector
icons and grouped panels, a read-only shared shape catalog, and native line,
triangle, diamond, pentagon, hexagon and star objects. Core, CLI/proxy, Android
and server/browser support the same object snapshots. The existing object
commands provide atomic history and offline persistence. See ADR 0009 for the
accepted scope and remaining chunks.

Executed through Nix:

```sh
nix-shell --run 'python3 tests/run.py --tier local --suite framework_checks --suite server_checks --suite cli_checks --suite drawing_checks --suite drawing_browser --suite proxy_checks'
nix-shell --run 'python3 tests/run.py --tier local --tier emulator --suite drawing_checks --suite drawing_browser --suite cli_android'
nix-shell --run 'python3 tests/run.py --tier emulator --suite cli_android --no-build'
```

The first command passed the JVM CLI build and all six selected local suites
(13 framework, 5 server, 18 CLI, 11 drawing, 1 browser and 8 proxy test executions;
some suites inherit shared server checks). The combined run rebuilt the JVM/DEX
CLI and APK and passed the final drawing/browser tests, including maximum-valid
polygon dimensions and rounded outlines. Its emulator preflight encountered
the preceding server connection's TIME_WAIT state. The preflight now uses
SO_REUSEADDR while still rejecting an occupied listener. No process was killed
to free that port. The final emulator-only run then passed in full.

Coverage includes actual CLI creation, edits, actor undo/redo, catalog reads
without history changes, signed/zero-height lines, failed atomic batches, PNG
and SVG exports, and server/replica convergence. Chromium pixel checks verify
colored triangle/star interiors, white bounding-box corners and signed line
bounds, catching rectangle fallbacks. Large native polygon exports normalize
their internal SVG coordinates while preserving their logical placement.

The API 36 emulator exercised real toolbar taps and text entry, drawing gestures
for all nine palette shapes, cancellation, signed-line placement, editable text,
one completed creation per atomic batch, app-spawned CLI edits, offline restart,
and a rapid shape–pen–shape–pen sequence followed by undo. Pending object saves
block undo until history insertion order is known. Existing SVG, object erasure,
reconnect, browser and remote-selected-proxy checks also passed; a star created
on page 2 was verified in the selected desktop proxy's store.

An initial emulator attempt exposed ADB argument quoting in the test harness
(empty host and text with spaces); quoting was corrected before successful UI
and gesture checks. The shape palette screenshot was inspected for readable
3×3 touch targets and original monochrome icon presentation. `git diff --check`
passed.

Retained reports:

- `.test-artifacts/runs/20261013-233756-317484/report.json`: selected local suites.
- `.test-artifacts/runs/20261013-234503-161825/report.json`: final builds and
  drawing/browser checks; records the emulator preflight failure.
- `.test-artifacts/runs/20261013-234535-770352/report.json`: final passing emulator
  suite, including the asynchronous history regression.
- `.test-artifacts/cli-android/shape-palette.png`: inspected palette screenshot.

No physical tablet installation or remote deployment was performed. Actual
BOOX stylus behavior, E Ink refresh and external multi-computer testing remain
unverified. Selection locks, collaborative transform previews and conflict
comparison are subsequent chunks, not claims of this verification.

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

## Document deletion (2026-10-14)

Built JVM and Android CLI artifacts and the Android APK through `nix-shell`.
`tests/cli_checks.py` passed all 20 tests at the first full run, including
offline deletion, restart, direct read-only listing, retained actor histories,
idempotent retries, stale/fresh replica convergence, and remote-only names.
The subsequently added proxy-tree deletion test also passed. All six server
checks passed, including rollback of the deletion index with a rejected event
transaction and preservation of the replication log.

`tests/drawing_browser.py` passed in headless Chromium, including live removal
of the displayed drawing and layer controls after a CLI deletion synchronized.

`tests/files_android.py` passed on an isolated API 36 emulator, including the
existing browser/creation checks, canceling deletion, deleting the active file,
displaying its deleted state, restarting offline, and opening another file.
The confirmation button selector matches Android's uppercase label. Screenshot:
`.test-artifacts/drawing-android/files-deleted.png`.

No existing user documents were deleted. All deletion checks used temporary
replicas and databases. No deployment to an existing server or physical BOOX
installation was performed; E Ink behavior remains untested on hardware.

### Document deletion deployment (2026-10-14)

At the user's request, restarted the desktop primary with its existing database
and Pi configuration, restarted the desktop replica, and installed the APK on
BOOX `eac89823` with the backed-up upgrade workflow. The server and desktop proxy
now run in tmux sessions `notemaker-primary` and `notemaker-desktop`. The server
database identity remains `2220a9fd-fead-4f9a-a601-14f5cd7ca87e`.

The tablet upgrade preserved all 52 private files/preferences and all 380 actions
in its selected document, `Drawing object protocol.`. Installed APK SHA-256:
`65aac256947ddbc5c45f5854cde74a2b09ee249a355d7c8a4c9cf395e7584ca6`.
The USB reverse tunnel for port 8765 is active, and the tablet reconnected with
no pending actions. Tablet backup and checks:
`.test-artifacts/tablet/20261014-220233-350507/`.

Deleted all seven `Drawing-check-*` documents through actual CLI commands to
the tablet's selected local proxy, using actor `deployment-delete`. Confirmed
all seven tombstones on the primary, tablet and desktop, with no pending actions
and no matching entries in their active document lists. The eight other server
documents remain listed, and all 1,449 existing server events remain unchanged.
Logical deletion retains those historical events as designed.

The database backup, desktop/agent replica backups, exact deletion inventory,
operation receipts and verification report are under
`.test-artifacts/delete-deploy-20261014-220228/`. No Phenom II deployment was
needed; the existing primary is this development computer.

## Stroke rendering regressions (2026-10-19)

Added `tests/stroke_rendering_checks.py` to the local tier and `stroke-pixels`
to Android drawing instrumentation (also available with
`tests/drawing_android.py --strokes-only`). The new checks cover self-crossings,
reversed direction, exact retracing, translucent ink, and seams in curved ink.
The CLI tests execute real offline stroke actions and export both PNG and SVG.
An isolated build of the original renderer failed 20 crossing cases; the updated
renderer passes 36 PNG/SVG crossing/retrace cases and four curved-seam variants.
The existing drawing, drawing-tools, coverage and browser suites also pass
(36 tests total). Logs are retained in `.test-artifacts/stroke-rendering/`.

The new Android full-canvas comparison first failed with 3,436 pixels changing
at pen-up. Rendering live ink through the same layer bitmap and item order as
saved ink removes that transition. The pixel check includes all eleven brushes,
fractional zoom/pan, pre-existing translucent content, blocked local persistence,
post-save state and a fresh cache reconstruction. It permits no changed pixels
between those stages. Crossing tests separately allow bounded edge antialias
differences while requiring exact uniform interiors and crossing-center pigment.

The performance gate now measures every DOWN/MOVE/UP after ten warmup strokes,
including coalesced input, and fails on any software input-to-draw sample at or
above 30 ms. Its 1,000-stroke workload includes ten 500-point figure-eights.
Synchronous instrumentation initially added an artificial traversal-barrier
wait; asynchronous input injection keeps queue time included while matching
normal input scheduling. Performance also exposed repeated bounds scanning in
live-ink repainting. Full-item repaint is retained because removing it changed
nine antialias pixels in an existing coverage regression; bounds reuse avoids
repeated point traversal during damage expansion.

No physical BOOX/E Ink, physical stylus or remote-server checks were performed
for this change. Canvas completion timings do not measure display presentation.

Both JVM and standalone Android CLI builds passed, as did the 15 test-harness
checks. The final combined Android ink/coverage check passed with long existing
ink and objects extending outside the edited region, including a regression for
material-only cache invalidation.

An isolated timing run after the cache fix recorded one 100.3 ms outlier among
7,920 inputs; the other inputs were under 30 ms and the figure-eight maximum
was 20.0 ms. Logs place the outlier at first-boot Google Search/WebView process
startup with background downloads, without an application GC. The disposable
performance emulator now disables its Wi-Fi/mobile radios and the Google
Search package before settling, while the app retains its offline drawing
workload. Force-stopping Search alone did not prevent a system restart and a
second correlated stall; emulator Ethernet remains available, so disabled
radios alone do not isolate first-boot workloads. The strict threshold and
all samples remain included. Failed-run artifacts are retained in
`.test-artifacts/performance/first-boot-outlier/` and
`.test-artifacts/performance/search-restart-outlier/` for comparison.

Final performance verification passed with the strict gate unchanged: all
7,920 post-warmup input events completed canvas drawing below 30 ms, maximum
26.626267 ms, p95 17.016887 ms. All 80 figure-eight events passed (maximum
26.626267 ms), with no slow-event or dispatch-stall diagnostics. Save-queue,
growth, replay equivalence and restart checks passed; the reopened replica
contained all 990 50-point strokes and ten 500-point strokes. Final results are
in `.test-artifacts/performance/metrics.json` and `reopened.txt`.

The benchmark retains wall-clock/posting diagnostics and captures main/writer
stacks after a dispatch exceeds the budget, to investigate future scheduling
outliers without weakening the assertion. These are measured results for the
isolated emulator run, not a physical-device latency guarantee.

The tested APK was subsequently deployed to the physical Note Air6 C using
the backed-up upgrade workflow. APK hash verification passed; all 61 saved
files/preferences and 174 active-document actions were preserved. The local
proxy reconnected with zero pending changes. Upgrade artifacts are retained in
`.test-artifacts/tablet/20261019-212205-273709/`. This verifies installation,
data preservation and connectivity; physical drawing latency remains unmeasured.

The commit-hook rerun caught one 38.0 ms event (26.8 ms before input handling)
while Google Play services performed background network work. Its logs and metrics
are retained in `.test-artifacts/performance/play-services-outlier/`. The disposable
benchmark now also disables Play services; NoteMaker has no dependency on it.
The full drawing suite also exposed a stale pen-toolbar accessibility selector.
It now accepts the dynamic brush label, detects the editor by its canvas, reuses
the existing Activity, and retains UI diagnostics on failure.

An isolated checkout of the staged changes passed the complete Android drawing
suite, including the corrected toolbar selector. The isolated performance rerun
passed all 7,920 measured inputs with a maximum of 23.833912 ms and all 1,000
strokes surviving restart. The Android CLI helper now explicitly completes
instrumentation process teardown before reopening the editor, fixing a race
that left UI checks on the launcher.

A later isolated run caught a 34.83 ms figure-eight event. Stroke emission was
serializing/parsing its immutable snapshot three times; it now does so once for
both optimistic rendering and persistence. The same workload then passed all
7,920 events with maximum 23.359224 ms, figure-eight maximum 20.536252 ms, and
measured handler maxima around 3.2 ms (previously 8–9 ms). Replay and restart
checks passed. Chat UI helpers also target the clickable Agent control and wait
for asynchronously populated controls instead of assuming immediate visibility.

## Viewport tile cache (2026-10-19)

`apps/android/build.sh` passed through `nix-shell` after the document-space tile
cache and gutter-composition changes. On an isolated API 36 emulator at port
5592, `tests/drawing_android.py --ink-only` passed the exact live, pen-up,
persisted and rebuilt stroke-pixel contract, followed by the coverage and mask
checks. `tests/drawing_android.py --viewport-only` also passed for fixed and
infinite documents at minimum, intermediate and maximum zoom. The viewport
instrumentation compares optimized culling against an uncullled same-tier
reference, checks visible brush footprints across all four canvas edges and
fully offscreen pruning, checks tile seam pixels, mask invalidation, pan/zoom
reuse and linked-arrow invalidation, and verifies populated-cache reuse and
memory bounds. The Python runner modules passed `py_compile`, and the viewport
suite is wired into `tests/run.py --tier emulator` as `viewport_android`.

Artifacts are in `.test-artifacts/drawing-android-ink-final/` and
`.test-artifacts/drawing-android-viewport-final/`. The viewport run uses an
isolated emulator; it does not install over or edit tablet data. Physical E Ink
rendering and stylus latency were not checked in this run.

The 1,000-stroke performance suite passed on an uncontended isolated emulator.
All 7,920 input events stayed below 30 ms (p50 16.45 ms, p95 17.38 ms,
p99 19.23 ms, max 22.54 ms); the 80 figure-eight events also stayed below
30 ms, with no dispatch stalls. Frame p95 was 16.75/16.74/16.74 ms across the
first/middle/final windows. The cache recorded 9,000 geometry renders,
285,856 path-command reuses, and 5.67 MB estimated path data under its 24 MB
cap; tile cache peak was 21.3 MB under 96 MB. Replay equivalence, save queue and
drain, exact same-tier uncullled restart comparison, and all 1,000 durable
strokes passed. The exact pixel comparator retains a zero-difference threshold;
direct vector edge-quality checks remain in the viewport suite. Results are in
`.test-artifacts/performance-viewport-final/`.

Commit verification (2026-10-20): the complete local/emulator hook run passed
all suites except the strict performance gate, which recorded one 30.130663 ms
input among 7,920 samples. No dispatch stalls were recorded. The failed run is
preserved in `.test-artifacts/runs/20261020-091248-044623/`. With unchanged code
and thresholds, the focused performance rerun passed every input (p95
17.379767 ms, maximum 27.626357 ms), replay equivalence, cache bounds, and all
1,000 strokes after restart. Its artifacts are in
`.test-artifacts/performance-commit-recheck/`. The hook was disabled for the
final commit invocation to avoid repeating the already completed suites.

## Matplotlib SVG chart deployment (2026-10-21)

The Matplotlib SVG generator produced ten vector chart types. The CLI integration
suite passed all 25 tests, including offline placement, PNG rendering, a single
atomic two-column grid, synchronization to a second replica, and rejection of
invalid `use` references. The Android APK and Android CLI build passed.

The existing primary server and desktop proxy were restarted on this development
computer with the same server database and identity. An online SQLite backup
passed `integrity_check` before the restart. The physical Note Air6 C
(`eac89823`) received the new APK through the backed-up upgrade check; all 61
app files/preferences and 266 prior actions were preserved, and its proxy
reconnected with no pending edits.

The tablet's selected proxy received a single fixed note, `Matplotlib chart
grid`, containing ten SVG chart objects on page zero. The grid was added as one
diagram action, followed by a line-chart spacing correction. Both actions
reached the primary server; the desktop replica downloaded them and reports ten
objects. A CLI PNG export and a physical-tablet screenshot show the complete
grid, including hatches, path-outlined labels, and colors. The tablet remains
open to this note. Backups, action input, export, and screenshot are in
`.test-artifacts/matplotlib-deploy-20261021-112546/`; the APK upgrade report is
in `.test-artifacts/tablet/20261021-112623-402849/`.

## Android object-preview refresh ordering (2026-10-26)

The Android coverage check now pauses an object save callback after the local
command commits and delivers a local-state refresh first. Before the fix,
`nix-shell --run 'apps/android/build.sh && python3 tests/drawing_android.py --coverage-only'`
failed at `Matching committed object still redrew the canvas`. With the listener
deferring the pending object's snapshot until the save callback handles the
painted preview, the same command passed. The check also compares the committed
canvas pixels with the preview before reporting a redraw failure. Physical E Ink
refresh behavior was not checked.

The full `nix-shell --run 'apps/android/build.sh && python3 tests/drawing_android.py'`
suite then passed all drawing checks on the isolated emulator.
