# ADR 0020: Durable local replica checkpoints

Status: accepted on `perf/checkpoint-scheduling`, 2026-10-20.

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
