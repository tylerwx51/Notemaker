# Current 1,000-stroke resource sweep — 2026-10-21

The preferred `perf/selection-lazy-hit-test` revision (`ce6368b`) was measured on
the physical NoteAir6C with 1,000 synthetic strokes in an isolated app package.
The run preserved all 1,000 strokes after process restart and matched incremental
state to replay. The raw capture is under
`.test-artifacts/hillclimb/tablet-current-resource-sweep-1/`.

| Metric | Preferred, 100 ms live publish | Alternative, 150 ms live publish |
| --- | ---: | ---: |
| Input-to-app-draw p50 / p95 / p99 | 34.40 / 60.64 / 66.81 ms | 39.23 / 63.88 / 72.30 ms |
| App-draw p95, strokes 901–1000 | 31.79 ms | 44.74 ms |
| Window-frame p95, strokes 901–1000 | 91.13 ms | 91.09 ms |
| Process CPU for workload | 275.24 s | 281.43 s |
| Charged process writes | 36.47 MiB | 34.76 MiB |
| Logical process writes | 4.16 MiB | 4.05 MiB |
| Peak sampled process PSS | 278.5 MiB | 263.3 MiB |
| Tile cache at end | 20.31 MiB | 20.31 MiB |
| Save queue maximum | 2 | 2 |

The alternative branch `perf/live-publish-150ms` (`1583523`) changes only the
in-progress stroke publish interval. It keeps local redraw on every input and
the final publish at pen-up. Both runs passed replay and reopen checks, but the
150 ms interval trades a modest 1.71 MiB reduction in charged writes for worse
input-to-app-draw latency, especially in the final window. Keep the 100 ms
interval. This also retains the cadence recorded in ADR 0001.

Process counters were sampled every 100 strokes, outside input timing. PSS
samples include garbage-collector variation; the observed peak difference is
not evidence of a stable memory reduction. Charged writes are cumulative kernel
accounting, not a direct flash-wear or fsync-latency measurement. Synthetic
MotionEvents and Android frame timing do not measure physical pen sampling or
E Ink scanout. The isolated package leaves personal app data untouched.
