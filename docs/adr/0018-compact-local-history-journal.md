# ADR 0018: Compact local history changes in journal records

Status: accepted on `perf/compact-actor-history`, 2026-10-20.

## Context

The local journal keeps a complete in-memory replica for fast reads, but each
durable edit used to serialize the entire changed history list in its metadata.
The Android freehand editor saves a growing top-level `undo` list after each
stroke; object and diagram actions grow actor-specific undo/redo stacks. That
made journal bytes increase with the total prior history even though each edit
usually changes only its tail.

## Decision

Journal records may now contain `lists` splices for the editor's top-level
`undo` and `redo` arrays and `history` splices for actor-specific undo/redo
stacks. A splice records the number of tail entries to pop and the new tail to
append. The existing in-memory state and CLI snapshot formats stay unchanged.
When actor metadata has fields other than undo/redo that change, the writer
falls back to the previous full `meta.actors` record. Readers still accept
older journal records, so a replica can contain both formats. Every record
retains its revision and CRC transaction boundary; replay validates the splice
against the reconstructed list length. Failed or interrupted transactions keep
the existing recovery behavior.

## Evidence and limits

CLI and proxy suites passed, including actor-specific undo/redo, concurrent
clients, crash truncation and checksum rejection. A focused test checked that
40 editor settings updates and 40 object actions keep record sizes bounded and
replay after restart. An actual CLI compatibility check opened a journal
written by the baseline build, appended new compact records, then restarted
and verified both actor history and the editor undo list.

On the physical NoteAir6C, the same isolated 1,000-stroke workload reported
62.0 MiB charged process writes at baseline and 38.8 MiB with both splices,
a 37.4% reduction. Logical process write characters fell 80.7%. Stroke
input-to-draw p95 was 82.2 ms at baseline and 80.6 ms with splices; one run
per build does not establish a latency gain. Actor history splices alone had
essentially unchanged stroke I/O because freehand input uses `append` and
top-level editor settings. Raw runs stay in `.test-artifacts/hillclimb/` and
sanitized summaries are in `docs/design/performance/`.

The splice still scans the shared list prefix and the editor still sends its
full undo list to the local proxy. Process writes also include Android runtime
and filesystem activity. A separate future change can reduce command payloads
or physical display latency without changing this durable format.
