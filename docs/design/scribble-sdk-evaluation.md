# BOOX Scribble SDK evaluation

Exploration recorded 2026-10-18 on `explore/scribble-sdk`.

## What the extracted Notes app shows

The BOOX Notes APK routes touch events through `ScribbleHandler` and BOOX SDK
types such as `TouchData`, `Shape`, `ShapeCreateArgs`, and `NoteDrawingArgs`.
Page rendering is submitted through NoteCore requests such as
`RenderNotePageThumbnailRequest`. The APK does not contain the renderer source;
the README for the extraction records that firmware supplied code is outside
the APK. This is architecturally different from NoteMaker's shared
`Render.drawStroke`, which resamples points, creates a closed swept ribbon, and
asks the platform surface to fill that outline.

## Emulator result

The extracted Notes APK installs successfully on the project's API 36 x86_64
emulator. Launching its exported `ScribbleTestActivity` fails during application
startup. Logcat reports missing `libc++_shared.so`; a background initialization
also reports missing `libengine_sdk.so`. The APK contains no native libraries,
and BOOX firmware-provided system components are not present in the emulator.
Therefore the SDK renderer cannot be exercised or benchmarked on this emulator
from the extracted APK alone. The emulator test was isolated on port 5588; the
package was removed after the check. The physical NoteAir6C was not modified.

## NoteMaker emulator performance baseline

Ran `nix-shell --run 'python3 tests/performance.py'` on this branch. The existing
suite drew 1,000 strokes with 50 pressure-bearing points each on the isolated
API 36 emulator. It passed latency and growth budgets, incremental/full replay
equivalence, cached/uncached canvas pixel comparison, and persistence after
process restart.

| Metric | First window | Middle window | Final window |
| --- | ---: | ---: | ---: |
| Input-to-draw p95 | 16.78 ms | 16.90 ms | 16.88 ms |
| Window frame p95 | 16.74 ms | 16.75 ms | 16.75 ms |
| Dispatch p95 | 15.78 ms | 13.80 ms | 12.58 ms |

The complete generated run took 153.05 seconds, final save drain was 752 ms,
maximum save queue was 1, and the ink cache had zero rebuilds. These figures
measure NoteMaker's existing renderer only; they are not a BOOX SDK comparison
or a physical E Ink latency measurement. Raw metrics and logs are retained in
the ignored `.test-artifacts/performance/` directory.

## Repository checks during this exploration

The repository pre-commit hook ran the local and emulator tiers. Builds,
topology, CLI, chat UI, browser, coverage, selection, drawing tools, and the
performance suite passed. The first two-emulator integration run timed out;
`drawing_android` and `files_android` then saw instrumentation process crashes
while emulator load was high. Sequential reruns passed integration and the file
browser. A sequential drawing rerun still failed on its gesture-routing step:
app startup reported `A proxy already owns this replica`. This is recorded as
an unrelated test/runtime failure, not attributed to the SDK investigation.

## Potential value and constraints

If BOOX supplies a supported redistributable SDK artifact that runs without
firmware libraries, it could be evaluated as an Android-only rendering backend
for pressure-sensitive ink, native input handling, and E Ink-aware refresh.
Those are plausible benefits based on the APIs used by Notes, not verified
performance claims.

Adopting it as the renderer would introduce device/vendor coupling. NoteMaker's
CLI, desktop PNG renderer, browser viewer, and shared action model must continue
to render the same point-based stroke protocol. The SDK would need to consume
that protocol or sit behind an Android-only adapter with the shared renderer
remaining authoritative for exports and non-BOOX builds. Emulator coverage
would also need a fallback renderer or a mockable SDK boundary. Without a
standalone SDK distribution and license, integration is not currently a
workable path; use the extraction as a behavioral reference and investigate
the ribbon fill artifact in NoteMaker's own renderer instead.
