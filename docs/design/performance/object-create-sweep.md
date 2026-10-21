# Physical object-creation sweep — NoteAir6C, 2026-10-20

The isolated interaction suite now creates 40 rectangles through the Android
editor's normal `persistObject` path. It discards five warmups, waits for each
object to appear in a new window and Canvas draw, then verifies all 40 objects
and editor-history entries in the durable local replica. This branch changes
only the benchmark; it is the control for object-creation latency experiments.

| Measure | Run 1 | Run 2 |
| --- | ---: | ---: |
| Input to updated window draw p95 | 68.27 ms | 66.81 ms |
| Input to updated Canvas draw p95 | 83.79 ms | 79.35 ms |
| Measured window frames | 86 | 84 |
| Window draw phase p95 | 18.68 ms | 17.23 ms |
| Charged writes | 744 KiB | 744 KiB |
| Process CPU | 3.95 s | 3.80 s |
| PSS before/after mode | 195/238 MiB | 195/238 MiB |

Object and stroke undo/redo in the same runs were roughly 38–42 ms p95 to
window draw. The create path first redraws a transient saving state, then
performs a durable command, reads a separate snapshot, saves editor history,
and redraws the object. The timings suggest room to remove redundant frames
and reads, but do not assign all latency to any one step. They exclude physical
E Ink scanout. Raw APK hashes and metrics are under
`.test-artifacts/hillclimb/tablet-object-create-baseline-{1,2}/`.
