# Return the durable object-history snapshot — NoteAir6C, 2026-10-20

`perf/history-no-pending-frame` removed the unnecessary redraw when object
undo/redo starts, but the editor still requested a separate snapshot after
the local command completed. The proxy already had the post-edit journal
state. This branch exposes `undo snapshot` and `redo snapshot` through the
CLI/proxy and returns that materialized state with the operation ID. Ordinary
`undo` and `redo` retain their existing output. The Android editor uses the
combined response for object history; stroke history keeps its existing
active-action path. All mutations still pass through the shared local Store.

Two isolated offline NoteAir6C runs used the same copied `notes` fixture and
alternated 60 object undo/redo actions. Six actions warmed the mode; final
object visibility and editor history were checked after the sweep.

| Measure | Parent runs | Candidate runs |
| --- | ---: | ---: |
| Input to updated window draw p95 | 60.44 / 60.50 ms | 42.74 / 41.25 ms |
| Input to updated Canvas draw p95 | 75.67 / 76.41 ms | 57.52 / 59.32 ms |
| Measured window frames | 65 / 62 | 65 / 63 |
| Charged writes | 1,128 / 1,128 KiB | 1,128 / 1,128 KiB |
| Process CPU | 5.49 / 5.44 s | 5.31 / 5.35 s |
| PSS before/after mode | 184/232, 184/233 MiB | 184/226, 183/225 MiB |

This is a repeatable 18–19 ms p95 history improvement with no extra writes.
The CPU and sampled PSS differences are smaller and sensitive to collection
timing; they do not establish resource savings. Layer and page switching stayed
near the parent measurements. Timing ends at Android app/window draw and does
not include E Ink scanout. CLI tests verified the returned snapshot against a
separate snapshot command, actor history, and restart durability. Raw APKs and
metrics are under `.test-artifacts/hillclimb/tablet-history-return-snapshot-{1,2}/`.
