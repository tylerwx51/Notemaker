# Return the durable stroke-history snapshot — NoteAir6C, 2026-10-20

Stroke undo/redo appends an active-state action through the shared local
replica, then previously requested a separate snapshot before redrawing the
editor. This branch exposes `append snapshot FILE|-` through the CLI/proxy and
uses its post-edit snapshot for stroke history. Plain `append` retains its
existing output. The action, actor history, and journal durability semantics
are unchanged; the editor still works offline.

Two isolated NoteAir6C runs used the same copied `notes` fixture and the
60-action stroke undo/redo sweep introduced on `perf/stroke-history-sweep`.
Six actions warmed the mode. Each sample waited for changed stroke visibility
and a new window/Canvas draw; the suite checked final durable stroke and
editor history state.

| Measure | Baseline runs | Candidate runs |
| --- | ---: | ---: |
| Input to updated window draw p95 | 60.17 / 60.39 ms | 40.10 / 40.28 ms |
| Input to updated Canvas draw p95 | 75.57 / 75.29 ms | 55.20 / 57.17 ms |
| Window draw phase p95 | 20.03 / 18.82 ms | 20.03 / 19.99 ms |
| Measured window frames | 65 / 64 | 67 / 64 |
| Charged writes | 1,040 / 1,044 KiB | 1,040 / 1,040 KiB |
| Process CPU | 5.28 / 5.37 s | 4.99 / 5.23 s |
| PSS before/after mode | 227/200, 227/201 MiB | 227/195, 227/195 MiB |

The 20 ms p95 improvement repeated without a meaningful I/O or frame-count
change. CPU and sampled PSS varied less consistently and do not establish a
resource saving. The app/window measurements exclude physical E Ink scanout.
Actual CLI calls verified the structured response against a separate snapshot
and after proxy restart. Raw APKs and metrics are under
`.test-artifacts/hillclimb/tablet-stroke-history-return-snapshot-{1,2}/`.
