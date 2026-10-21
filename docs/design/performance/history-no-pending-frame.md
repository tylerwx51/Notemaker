# Avoid the transient object-history frame — NoteAir6C, 2026-10-20

The editor previously redrew immediately after queuing undo or redo, before
the durable local command changed the model. It then redrew again with the
result. This candidate retains the pending guard and save count but waits for
the result before invalidating the editor. Errors still trigger a redraw and
remain visible. The edit does not change the CLI, replica, or history format.

The same isolated offline NoteAir6C interaction suite ran twice from the same
copied `notes` document. Each run alternated 60 object undo/redo actions,
discarded six warmups, and checked the final model and editor history. The
parent is a clean run of `perf/settings-single-frame`.

| Measure | Parent | Candidate 1 | Candidate 2 |
| --- | ---: | ---: | ---: |
| Object-history input to updated window draw p95 | 70.49 ms | 60.44 ms | 60.50 ms |
| Object-history input to updated Canvas draw p95 | 80.19 ms | 75.67 ms | 76.41 ms |
| Measured window frames across 60 actions | 123 | 65 | 62 |
| Per-frame command-issue p95 | 47.83 ms | 7.13 ms | 6.76 ms |
| Object-history charged writes | 1,120 KiB | 1,128 KiB | 1,128 KiB |
| Object-history process CPU | 5.34 s | 5.49 s | 5.44 s |

Layer-switch p95 was 25.77 ms in the parent and 26.07/26.47 ms in the two
candidate runs; page-switch p95 was 18.96 and 20.46/18.91 ms. The candidate
therefore removes roughly one transient frame per history action and improves
history latency without an apparent navigation tradeoff. Write and CPU changes
are too small to establish improvement. Sampled PSS around the history sweep
varied with Java collection timing and is not evidence of a RAM regression or
saving. These timings stop at Android app/window draw, before E Ink scanout.

The raw metrics and isolated APK hashes are under
`.test-artifacts/hillclimb/tablet-history-no-pending-frame-{1,2}/`.
