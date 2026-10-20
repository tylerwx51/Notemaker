# Local history journal experiment — 2026-10-20

All runs used the same physical NoteAir6C, isolated offline package, and
viewport-wrapped 1,000-stroke workload. Each result is one run, with all
strokes verified after restart. The sanitized series are
[`tablet-strokes-baseline.json`](tablet-strokes-baseline.json),
[`tablet-compact-history.json`](tablet-compact-history.json), and
[`tablet-compact-editor-history.json`](tablet-compact-editor-history.json).

| Measurement | Baseline | Actor history only | Actor + editor history |
| --- | ---: | ---: | ---: |
| Process charged writes through 1,000 strokes | 62.0 MiB | 61.8 MiB | 38.8 MiB |
| Process write characters | 23.0 MiB | 23.0 MiB | 4.4 MiB |
| Input-to-app-draw p95 | 82.2 ms | 81.2 ms | 80.6 ms |
| Peak sampled process PSS | 438 MiB | 442 MiB | 429 MiB |
| Final save drain | 993 ms | 1,049 ms | 1,019 ms |

The actor-only result exposed a workload mismatch: freehand strokes use raw
`append` operations and the editor's top-level undo list, so actor history
compaction did not affect their I/O. The final candidate reduced charged
writes 37.4% and process write characters 80.7% relative to baseline. There
is no demonstrated display latency improvement; differences of around 1 ms
between single runs are too small to trust. The input handler itself remains
fast relative to the completed window frames measured on this tablet.

The full-history-to-splice migration was checked with a journal produced by
the committed baseline CLI, then opened and edited by the candidate CLI.
Actor undo/redo and top-level editor undo list survived restart. CLI and proxy
suite results, including journal recovery checks, passed. See [ADR 0018](../../adr/0018-compact-local-history-journal.md)
for the durable format and compatibility behavior.
