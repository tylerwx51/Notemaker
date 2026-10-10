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
