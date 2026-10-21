# Local stroke-ack baseline — NoteAir6C, 2026-10-20

The current `perf/stroke-history-return-snapshot` build completed the isolated
1,000-stroke physical workload. All strokes survived process restart and the
incremental model matched complete replay. This run is the control for testing
whether local append acknowledgments need to invalidate the canvas.

| Measure | Current run | Earlier `perf/settings-single-frame` run |
| --- | ---: | ---: |
| Input-to-app-draw p95, all strokes | 60.80 ms | 60.90 ms |
| Input-to-app-draw p95, last 100 strokes | 32.00 ms | 32.06 ms |
| Window draw phase p95, last 100 strokes | 12.07 ms | 12.51 ms |
| Measured window frames, last 100 strokes | 629 | 628 |
| Charged writes | 36.30 MiB | 36.45 MiB |
| Process CPU | 277.1 s | 276.8 s |
| Peak sampled PSS | 296.4 MiB | 268.1 MiB |

The two latency, frame, CPU and I/O results align closely. Peak PSS comes
from eleven sparse samples and varies with collection timing; the single
higher sample does not establish a RAM regression. The workload measures
Android app/window draw before physical E Ink scanout. Raw measurements and
the isolated APK hash are in `.test-artifacts/hillclimb/tablet-stroke-ack-baseline-1/`.
