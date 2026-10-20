# Direct live stroke map — 2026-10-20

The Android canvas used to serialize a live `JSONObject` stroke and parse it
back into a Java map on every cached draw and on each persistence emission.
Branch `perf/live-stroke-map` builds the same map from the in-memory point
array. It retains each point's original `Number` object; promoting values to
`double` in the first two trials kept pixels but increased serialized journal
traffic. The final form keeps the shorter original numeric representation.

| Physical NoteAir6C, isolated 1,000 strokes | Bitmap-reuse baseline | Direct map, first | Direct map, repeat | Preserved numbers |
| --- | ---: | ---: | ---: | ---: |
| Input-to-app-draw p95 | 80.08 ms | 71.96 ms | 73.04 ms | 72.49 ms |
| Middle-window app draw p95 | 49.14 ms | 31.87 ms | 31.73 ms | 32.08 ms |
| Final-window app draw p95 | 53.47 ms | 49.64 ms | 51.37 ms | 50.72 ms |
| Final-window frame p95 | 92.93 ms | 92.97 ms | 93.14 ms | 92.94 ms |
| Peak sampled process PSS | 278 MiB | 290 MiB | 283 MiB | 274 MiB |
| Charged process writes | 38.9 MiB | 42.3 MiB | 42.6 MiB | 39.4 MiB |

The app draw improvement repeated, and the final numeric representation kept
I/O close to baseline. Android window frame p95 did not improve; the hardware
frame submission path remains a separate latency limit. These are synthetic
stylus events and app draw timings, not physical pen sampling or E Ink scanout.

The emulator ink, mask, live/pen-up/persisted stroke and viewport pixel checks
passed. The final 1,000-stroke emulator gate kept all 7,920 inputs under
30 ms (p95 17.22 ms), with replay and restart checks passing.

Sanitized measurements are in [`tablet-live-map.json`](tablet-live-map.json),
[`tablet-live-map-repeat.json`](tablet-live-map-repeat.json), and
[`tablet-live-map-preserve-numbers.json`](tablet-live-map-preserve-numbers.json).
Raw captures stay under `.test-artifacts/hillclimb/`.
