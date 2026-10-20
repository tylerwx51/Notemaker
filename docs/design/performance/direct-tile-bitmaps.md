# Direct cached tile bitmap submission — 2026-10-20

Branch `perf/direct-tile-bitmaps` draws each resident tile bitmap with a cached
matrix and a clip to its document-space core. The full bitmap, including its
gutter, remains available to filtering. The preceding renderer used a cached
bitmap shader on a rectangle. Tile painting, cache membership, and document
actions are unchanged. Emulator viewport, seam, live/settled ink, mask, and
regional repaint pixel checks passed.
The emulator strict 1,000-stroke gate also passed: all 7,920 measured inputs
reached an app draw under 30 ms (p95 17.20 ms, maximum 24.02 ms), with replay
equivalence and 1,000 durable strokes after restart.

| Physical NoteAir6C, isolated 1,000 strokes | Shader baseline | Direct bitmap | Direct bitmap repeat |
| --- | ---: | ---: | ---: |
| Overall input-to-app-draw p95 | 68.7 ms | 66.6 ms | 67.6 ms |
| Final-window input-to-app-draw p95 | 51.7 ms | 49.2 ms | 50.5 ms |
| Final-window window-frame p95 | 91.1 ms | 90.9 ms | 91.0 ms |
| Final-window app draw p95 | 30.7 ms | 29.1 ms | 30.3 ms |
| Final-window command issue p95 | 49.9 ms | 49.6 ms | 49.6 ms |
| Peak sampled process PSS | 244 MiB | 246 MiB | 273 MiB |
| Charged writes | 37.2 MiB | 36.6 MiB | 36.8 MiB |

The app-draw gain repeated but is modest. Window-frame latency did not improve
materially: command issue remains near 50 ms p95. The second candidate run's
higher sampled PSS shows that this change has no established memory benefit.
Both candidate runs passed incremental/full replay agreement and confirmed all
1,000 strokes after process restart. These are synthetic app timings before
physical stylus sampling and E Ink scanout. Sanitized results and exact APK
hashes are in [`tablet-direct-bitmaps.json`](tablet-direct-bitmaps.json) and
[`tablet-direct-bitmaps-repeat.json`](tablet-direct-bitmaps-repeat.json); raw
captures remain under `.test-artifacts/hillclimb/`.

On the same eight-target disposable eraser archive, partial/whole-item
feedback p95 was 30.4/74.4 ms on this branch versus 35.9/70.5 ms in the
earlier live-map branch. Updated-draw p95 was 122.8/124.5 ms versus
129.7/127.4 ms. All erases were undone and source geometry restored. With only
eight targets per mode, these values are too sparse to attribute a small
difference to bitmap submission; they show no clear erase regression. The
note-free comparison is in
[`erase-direct-bitmap-comparison.json`](erase-direct-bitmap-comparison.json).
