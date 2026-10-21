# Generic object-put snapshot response — NoteAir6C, 2026-10-20

This experiment adds `object put snapshot FILE|-` to the structured CLI and
proxy. The Store returns the post-edit state with the atomic object action,
and the editor uses that snapshot rather than requesting a separate read.
The generic proxy path still reads state before and after executing the CLI
command for notifications. All 40 created objects and editor-history entries
remained durable in both isolated NoteAir6C runs. CLI, proxy and Android builds
passed; actual CLI calls matched the returned snapshot with a separate read
and after proxy restart.

| Measure | Parent runs | Candidate runs |
| --- | ---: | ---: |
| Input to updated window draw p95 | 62.99 / 60.74 ms | 63.35 / 63.48 ms |
| Input to updated Canvas draw p95 | 86.47 / 79.27 ms | 83.80 / 78.82 ms |
| Measured window frames | 43 / 43 | 43 / 43 |
| Charged writes | 748 / 744 KiB | 744 / 744 KiB |
| Process CPU | 3.70 / 3.63 s | 3.32 / 3.55 s |

The CPU difference is small and variable, and there is no display-latency
gain. This branch is kept as an experiment, not promoted. A direct proxy path
can be measured separately. Timings end at Android app/window draw before
E Ink scanout. Raw data and APK hashes are under
`.test-artifacts/hillclimb/tablet-object-create-return-snapshot-{1,2}/`.
