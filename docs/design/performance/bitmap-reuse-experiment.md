# Reuse settled tile bitmaps — 2026-10-20

Branch `perf/reuse-tile-bitmaps` repaints a changed tile into its existing
bitmap after clearing it. The stroke and item traversal, paint order, clipping,
and anti-aliasing remain the same. A tile still allocates on first use or after
LRU eviction. This change targets graphics allocation churn, not the number of
items painted.

| Physical NoteAir6C, isolated 1,000 strokes | Before reuse | Reuse run 1 | Reuse run 2 |
| --- | ---: | ---: | ---: |
| Peak sampled process PSS | 404 MiB | 292 MiB | 278 MiB |
| Peak sampled graphics PSS | 228 MiB | 88 MiB | 89 MiB |
| Input-to-app-draw p95 | 81.37 ms | 80.52 ms | 80.08 ms |
| Final-window app draw p95 | 54.71 ms | 54.07 ms | 53.47 ms |
| Final-window frame p95 | 92.84 ms | 93.02 ms | 92.93 ms |
| Charged process writes | 38.6 MiB | 38.7 MiB | 38.9 MiB |

The two reuse runs retained all 1,000 strokes after restart and performed
about 8,200 in-place tile repaints each. The tile cache held about 20 MiB of
live tile pixels in both builds; the reduction is in Android's graphics
accounting beyond that cache estimate. The runs establish a large sampled RAM
reduction, while the small latency differences do not establish a speed gain.
The roughly 80 ms input-to-app-draw p95 and 93 ms frame p95 remain open.

The API 36 emulator's ink, mask, viewport and exact cached-versus-unculled
pixel checks passed. Its 1,000-stroke gate kept every measured input under
30 ms (p95 17.35 ms), with 9,500 in-place repaints and restart verification.
These checks exercise app rendering; they do not measure E Ink scanout or prove
hardware GPU pixels are visually identical in every BOOX rendering mode.

Sanitized results are in [`tablet-memory-breakdown.json`](tablet-memory-breakdown.json),
[`tablet-bitmap-reuse.json`](tablet-bitmap-reuse.json), and
[`tablet-bitmap-reuse-repeat.json`](tablet-bitmap-reuse-repeat.json). Raw tablet
artifacts stay private under `.test-artifacts/hillclimb/`.
