# Current preferred-build stroke baseline — NoteAir6C, 2026-10-21

The preferred `perf/selection-lease-refresh` build completed two isolated
1,000-stroke physical-device runs. Each run dispatched 7,920 measured stylus
inputs, checked incremental/full replay equality, restarted the process, and
confirmed all 1,000 strokes. A fresh run of its immediate parent
`perf/fill-no-pending-frame` used the same workload as a control.

| Measure | Preferred 1 / 2 | Immediate parent |
| --- | ---: | ---: |
| Input to app draw p95 | 61.6 / 62.0 ms | 61.9 ms |
| Last-window app draw phase p95 | 13.1 / 12.5 ms | 12.7 ms |
| Last-window full frame p95 | 91.6 / 91.7 ms | 91.9 ms |
| Process CPU, strokes 0–1,000 | 291.9 / 293.4 s | 291.9 s |
| Charged writes before final drain | 37.1 / 37.2 MiB | 37.3 MiB |
| Tile repaints | 8,050 / 8,092 | 8,044 |
| Peak sampled PSS | 256 / 248 MiB | 255 MiB |
| Post-GC PSS | 131 / 186 MiB | 187 MiB |

The selected build is similar to its immediate parent for sustained stroke
latency, CPU, tile work, and I/O. Both current runs used more CPU than two
older `perf/render-prune-on-model-change` runs (about 283 s), but the paired
parent also used about 292 s; the lease-refresh change does not explain that
difference. The first preferred run's 131 MiB post-GC PSS came from a graphics
allocation drop that did not repeat, so it is not evidence of retained-memory
savings. The tile cache held about 20.3 MiB of live pixels in each run.

The sanitized second-run series is in
[`selection-lease-strokes-1000.json`](selection-lease-strokes-1000.json).
Private raw metrics and APK hashes are under
`.test-artifacts/hillclimb/tablet-{selection-lease-strokes-1000-{1,2},fill-no-pending-strokes-1000-control-1}/`.
These synthetic timings exclude physical stylus sampling and E Ink scanout;
PSS is sampled between stroke groups, and charged I/O is not flash latency.
