# ADR 0020: Durable local replica checkpoints

Status: accepted on `perf/checkpoint-relay-dedup`, 2026-10-20.

## Context

The physical NoteAir6C cold-start fixture has a 111-byte base replica and a
4.6 MiB journal with 847 records. Root validation reconstructs the journal
before the editor's first snapshot and takes about 970 ms. The existing WAL is
append-only and must retain all committed events, provisional operations,
relay order, actor histories, and document metadata.

## Decision

The selected local proxy exposes a `checkpoint` maintenance command through
the shared `Store` and journal lock. It serializes the complete current state
to a temporary file and syncs it, atomically replaces the base, syncs the
directory, then truncates and syncs the WAL. Its revision does not change. A
crash after base replacement but before truncation leaves old WAL records;
replay skips records whose revisions are already in the base. If replacement
does not happen, the old base and WAL remain usable. New edits continue to
append transactions after the checkpoint.

When the current `relay` is exactly the committed `events` followed by
`pending`, the checkpoint stores it as an empty list. The reader already
reconstructs that exact list, avoiding a second copy of operation geometry.
For a reordered relay containing exactly the same uniquely identified
operations as `events` and `pending`, a later checkpoint format stores the
ordered IDs with `relayFormat: "ids"` instead of duplicate operation geometry.
The reader expands and validates these IDs before replay. A relay with extra
or different entries still stays in full. This schema addition requires
updated app and CLI readers for ID-order checkpoints; older readers reject
them rather than silently changing relay order.
A checkpoint with repeated snapshots of one stroke stores each snapshot's
exact shared point prefix as a count, followed by only its remaining points.
`checkpointPointFormat: "stroke-prefix-v1"` identifies this checkpoint-only
encoding. `checkpointPointPrefixes` contains one count per stroke action in
`events` then `pending`, including strokes nested in batches. The reader
validates and expands the prefixes before relay reconstruction and model
replay. The in-memory actions, WAL, and CLI output keep complete point lists.
Older app and CLI readers cannot open a checkpoint using this format, so a
replica must be read with matching versions when sharing or rolling back.
During WAL replay, committed operations and metadata still apply in revision
order, but the reader rebuilds the effective model once after the last record.
Live transactions continue to update the cached model as each edit commits.

## Consequences

The checkpoint requires serialization and disk writes, so Android queues it
through the shared local proxy after the editor becomes hidden, and only when
its WAL is at least 1 MiB. It runs after queued saves. A foregrounded editor
skips work that has not begun. Android can kill the process during background
work; the old WAL or checkpointed base remains recoverable. The CLI command
remains available for explicit maintenance. Backups still need a consistent
base-plus-journal pair under the document lock or with writers stopped.

The proxy uses its persisted document-name index to skip a redundant full-base
parse during discovery, while still validating the base in `Store.read`. On a
copied physical-device note, checkpointed process-cold launches were 1,385 and
1,401 ms median versus 1,653 ms with the original WAL. Without this index,
checkpointed launch regressed to 1,801 ms. CLI and proxy checks passed,
including restart, actor history, and an interrupted-truncation simulation.
On the physical NoteAir6C, a 4.6 MiB WAL checkpoint took 298–305 ms wall time,
470–477 ms process CPU, and 2.42 MiB charged writes. Sending the editor home
checkpointed the copied note; subsequent process-cold launches were 1,416 and
1,443 ms median versus 1,660 ms without a checkpoint on the same APK. The
physical E Ink scanout remains unmeasured. See
`docs/design/performance/checkpoint-scheduling.md`.

With the later relay omission and fast JSON parser, the same copied note's
checkpointed base shrank from 2,391,204 to 1,216,522 bytes. Physical
process-cold launch medians were 1,094 and 1,067 ms, versus 1,171 and
1,165 ms with the full relay and same parser. The Android background workflow
produced the compact base and launched in 1,120 ms median. See
`docs/design/performance/checkpoint-relay-dedup.md`.
On a later copied note with a 6.0 MiB WAL, the single post-replay rebuild
reduced process-cold launch median by about 24–54 ms across two runs per
branch, with unchanged logical reads and sampled PSS. See
`docs/design/performance/journal-replay-batch-effective.md`.
On a copied note whose relay contained the same 548 operations in another
order, ID-order storage reduced its checkpointed base from 5,114,942 to
2,738,803 bytes. Two physical process-cold launch medians improved from
1,905/1,922 to 1,737/1,755 ms, with lower sampled PSS and logical reads.
Physical checkpoint writes fell from about 5.15 to 2.76 MB and wall time
from 524/531 to 334/359 ms. See
`docs/design/performance/checkpoint-relay-id-order.md`.
On that same copied note, point-prefix storage reduced the base from
2,738,803 to 1,215,466 bytes. Two physical process-cold launch medians were
1,592 and 1,587 ms, versus 1,730 ms on the preceding version; sampled PSS
fell by about 21 MiB. Physical checkpoint writes fell from 2,764,800 to
1,236,992 charged bytes, and checkpoint wall time from 334/359 to 183/193 ms.
See `docs/design/performance/checkpoint-point-prefixes.md`.
