# ADR 0013: Permanent document tombstones through local replicas

Status: accepted, 2026-10-14.

## Context

Users need to delete server documents from Android and the CLI, including while
offline. Removing server rows would invalidate replica cursors and allow an old
device to upload the deleted content again. Existing actor histories and atomic
content actions must remain intact.

## Decision

Add a standalone `document-delete` operation and shared `Store.deleteDocument`
transaction. The CLI exposes `document delete`; the Android Files browser uses
the same command after a long press and confirmation. Both mutate only their
selected proxy. The operation is durably queued offline and uses ordinary sync.

A tombstone is permanent and wins over all content edits and creation, regardless
of replay order. Retain logs and actor histories, but clear the effective drawing,
hide deleted names from user-facing lists, and reject new edits and undo/redo
once deletion is known. Display a deleted state if the document is already open.
Repeated deletion succeeds without generating another operation. Deleting a
name not present locally is allowed, supporting known server-only documents.

The server indexes deleted names transactionally with the event insertion and
exposes them separately in `api/info`. Keep replication endpoints available and
acknowledge stale content uploads so disconnected devices can converge without
stuck queues. Local proxy registries keep tombstoned documents in their sync set.
All peers must be upgraded to understand the operation before it is used.

## Consequences

Deletion is logical, not a storage reclamation or secure erasure mechanism.
Deleted names cannot be reused; restoration and document generations would
require a separate design. An erroneous name is also tombstoned permanently.
The UI states these consequences before confirmation. The CLI provides a direct
noninteractive command and documents its idempotent behavior.

## Verification

Actual CLI tests cover offline deletion, retained actor histories, idempotent
retry, proxy restart, direct read-only listing, server lists, stale replicas,
fresh replicas, remote-only names, blocked edits, and invalid nested deletion.
The isolated Android file browser test covers cancellation, deleting the active
document, opening another file, and persistence after process restart.
