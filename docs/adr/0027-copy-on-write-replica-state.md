# ADR 0027: Copy-on-write replica state in the Rust port

## Status

Accepted (2026-11-02), amended the same day (append-scope edits, copy-on-write
model indexes, in-process snapshots without `effectiveModel`). Refines ADR 0004
and ADR 0018 for the Rust port.

## Context

Java's journal handed out shallow copies. `Journal.snapshot()` copied the list
spines (`new ArrayList<>(list)`), but every operation map, and its stroke
points, stayed shared. `equals` on two such lists short-circuits on identity.
`Main.snapshot` returned a `Store.Snapshot` that carried the materialized
`Model`, so the editor's `refreshShared` never parsed `effectiveModel`.

The first Rust port used `serde_json::Value` throughout, and `Value::clone` is
a deep copy. On the API 36 emulator, a document with 1,000 offline strokes paid
about ten full deep copies of `pending` and `relay` per stroke. Every read
copied the journal state, Store's lease overlay copied it again, each edit
snapshot copied it, `difference` cloned `pending` twice, and the editor
re-parsed `effectiveModel` and deep-compared every stroke. The replica writer
fell behind the pen: the save queue reached 830 and the performance check timed
out, where Java's maximum queue was 1.

## Decision

Keep the JSON model and file formats. Share state the way Java did:

- **COW state.** `store::State.map` is an `Arc<ReplicaState>` (an
  `Arc<Map>` before ADR 0032 named the JSON). `DerefMut` uses
  `Arc::make_mut`, so a state is copied only when a holder mutates it.
  `Journal.state` holds the same `Arc`, so a read is an `Arc` clone.
- **Overlay inside the journal.** Store's process-local `leases` and
  `activeSessions` are kept as the last two keys of the cached journal state.
  They are updated in place under the journal lock. Java appended them to every
  returned copy. They are stripped wherever the state is persisted (the
  initial base file and checkpoints).
- **Edit snapshot without relay.** An edit works on a private copy of the
  state. Its `relay` is an empty placeholder at the same key position, because
  no edit reads `relay` and `difference` ignores it in `next`. `difference`
  borrows the pending operations instead of cloning them.
- **Snapshot carries the model.** `cli_main::snapshot_state` returns a
  `State` of kind `Snapshot` that carries the model, like `Store.Snapshot`.
  `LocalProxy::snapshot_state` is the in-process `snapshot` command.
  `AppReplica::read` returns it, and `MainActivity.refresh_shared` takes the
  model from it through the `SharedState` trait. Unchanged strokes then keep
  their `Arc`, and `entity()`'s pointer-equality fast path applies again.
- **Reply reuse.** The editor reads the same revision twice per edit: once
  after the write, and once on the change notification. `LocalProxy` reuses
  the last in-process reply when the journal map (by identity), the previews
  and the lease expiry state are unchanged.

### Amendment (2026-11-02): no O(document) work per save

The first version still cost O(document) per save. On the physical Note
Air6 C, 1,000 offline strokes left the save queue at 655 (Java: 1) and the
run timed out. A host benchmark at 1,000 strokes showed where the time went.
Each edit deep-copied the state for the edit (2.3 ms), deep-compared
`pending` (1.5 ms) and freed the copy (1.8 ms). Each read deep-copied every
stroke into `effectiveModel` (2.8 ms). About ten `Model` clones per stroke
copied every index (0.14 ms each).

- **Append-scope edits.** `journal::EditScope::Append` is used by Store's
  own action edits: `append`, `appendWithSettings`, `commit*`, `history` and
  `deleteDocument`. These edits never read `events`, `pending` or `relay`.
  They push new operations and change actor histories only by popping and
  pushing stack tails. The edit gets `events`, `pending` and `relay` as empty
  lists. Its new `pending` entries are added after the existing ones.
  `actors` is moved out of the cache into the edit, not copied, and moved
  back after the commit. The history splice compares entries by identity, the
  address of the entry's first key, which survives moves. This is what Java's
  `equals` on shared references amounted to. The cache is unshared
  (`Arc::make_mut`) before identities are recorded, because a copy gives
  every entry a new address. A rejected edit returns `actors` if it is
  unchanged by identity. Otherwise the cache reloads from disk on the next
  access. Other edits (`Store.edit`, sync, accept, CLI document create) keep
  the Full path, a private copy compared with the cache. Full-path
  `historyDifference` now borrows the stacks instead of cloning them.
- **Copy-on-write model indexes.** `Model`'s maps are `model::Shared`
  (`Arc` with `make_mut`). A clone is O(1), and the first mutation copies only
  that map. This is cheaper than Java's spine copies. Equality short-circuits
  on identity.
- **In-process snapshots without `effectiveModel`.** Java's comment said
  in-process snapshots retain the materialization and JSON clients use
  `effectiveModel`. `LocalProxy::snapshot_state` (the app's `read`) and the
  new `LocalProxy::execute_with_snapshot` therefore leave out the JSON and
  return a `State` that carries the model. The app's `object put snapshot`,
  object undo/redo and stroke undo/redo use it through
  `AppReplica::command_snapshot`. Before, the app re-parsed `effectiveModel`
  for every object save. JSON replies (CLI, HTTP, remote descriptor) are
  unchanged.
- **Verification.** Debug builds, or `NOTEMAKER_VERIFY_APPEND=1`, run the
  Full difference and apply next to every Append edit. A difference in delta
  bytes or resulting state panics. Parity, the local tier and
  `packages/core/tests/append_scope.rs` ran with the check on. The check
  found the identity-before-unshare bug while a sync held a read.

Measured (1,000 strokes, see `docs/verification.md`): host write at
100→1,000 strokes went from 3.3→9.3 ms to 3.1→3.3 ms. The remaining time is
mostly `fsync`. Two reads per stroke went from 0.3→5.7 ms to 0.06→0.16 ms.
Object commits measured 3.0→3.3 ms. On the emulator the maximum save queue
went from 34–45 to 0 and the drain from 1.8–2.3 s to 0.57 s. On the Note
Air6 C the isolated synthetic run now completes. The save queue was 1 at every
100-stroke sample, against 2→655 before. The peak was 11 and the drain took
0.92 s. Stroke latency p50/p95/p99/max was 24.8/48.3/65.0/81.8 ms. The Java
hardware baseline from 2026-10-20 was 50.2/82.2/—/128.8 ms with queue 3 and
drain 0.99 s. Hardware does not meet the emulator's 30 ms gate under either
build. Dispatch and draw p95 still rise over the run (input-to-draw p95 24→45
ms from the first to the last window), which is UI-thread and rendering work,
not the writer.

## Consequences

- Command output, journal bytes and checkpoints are unchanged
  (`packages/core/tests/parity.sh`, `cargo test -p notemaker_core`, local and
  emulator tiers).
- Append-scope edits rely on Store's action edits keeping to the scope's
  contract. A new Store edit that reads `pending`/`events`, or rewrites
  history entries, must use `Store::edit` (Full). The debug cross-check
  reports a violation as a delta/state mismatch.
- Per-save work is still O(entities) in places where Java's was too: the
  editor's undo list and its splice, `refreshShared`'s entity map rebuild,
  one model-index copy per edit while a snapshot is held, and an O(actor
  history) pointer scan per action edit. These have small constants.
- The in-process snapshot `State` has no `effectiveModel` key. In-process
  code must take the model with `store::model`, not from the map.
- The journal's cached map can contain `leases`/`activeSessions`. New code
  that serializes journal state must use `without_overlay`.
- Freeing large trees on a background thread was tried and rejected. Frees
  across threads contended with the writer's allocations, and the writer got
  slower.
