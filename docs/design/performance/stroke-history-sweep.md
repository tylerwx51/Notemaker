# Physical stroke-history sweep — NoteAir6C, 2026-10-20

The isolated interaction suite now measures stroke undo/redo separately from
object history. It seeds a pressure-bearing stroke through the shared local
`append` command, then alternates 60 editor undo/redo actions and discards six
warmups. Each sample waits for changed visibility and a new window/Canvas
draw. The suite checks the final stroke and editor history in the durable
replica. This branch changes only the benchmark; it is a baseline for a later
stroke-history implementation experiment.

| Measure | Run 1 | Run 2 |
| --- | ---: | ---: |
| Input to updated window draw p95 | 60.17 ms | 60.39 ms |
| Input to updated Canvas draw p95 | 75.57 ms | 75.29 ms |
| Measured window frames | 65 | 64 |
| Window draw phase p95 | 20.03 ms | 18.82 ms |
| Charged writes | 1,040 KiB | 1,044 KiB |
| Process CPU | 5.28 s | 5.37 s |
| PSS before/after mode | 227/200 MiB | 227/201 MiB |

Object-history window-draw p95 in the same runs was 40.60/38.66 ms. Stroke
history still uses a separate snapshot read after appending an active-state
action, which is a plausible cause of the difference but remains an inference
until a controlled candidate is measured. PSS moved with garbage collection,
so the before/after numbers do not establish a RAM saving. Timings stop at
Android app/window draw before E Ink scanout. Raw artifacts are under
`.test-artifacts/hillclimb/tablet-stroke-history-sweep-baseline-{1,2}/`.
