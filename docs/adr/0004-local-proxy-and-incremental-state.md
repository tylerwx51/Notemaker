# ADR 0004: Local replica proxy, incremental state, and asynchronous ink

Status: accepted, 2026-10-12. Supersedes ADR 0003's full JSON replacement,
per-edit replay, Activity-owned synchronization and absence of a local service.

## Context

The physical Note Air6 C stalled and entered ANR after about a minute of drawing.
Both captured ANR stacks showed numeric JSON parsing in a full-document save on
the UI thread. Full stroke snapshots compounded repeated persistence and replay.
Agents need an endpoint for the same local state used by the editor. The desktop
at its currently configured LAN address is the primary server; the existing
SQLite database and its identity must be preserved.

## Decision

Keep the existing server protocol and actor histories. A local replica proxy
owns an in-process cache of the document's committed model and optimistic model.
Apply new local operations incrementally. On synchronization, apply only new
server events to the committed model and overlay the remaining outbox; never
replay the entire committed history on every edit. A cold process reconstructs
the cache once. File locks and journal offsets let standalone CLI processes
participate safely, and the proxy notices their new transactions.
Large catch-ups persist bounded groups of complete operations, with the cursor
advancing in each transaction. Outbox acknowledgements are removed only after
complete catch-up; interrupted downloads resume from their durable cursor.

Retain each legacy `document-*.json` as a migration baseline. Append subsequent
transactions to its `.json.journal` sibling, with a length, UTF-8 JSON delta,
CRC32, and fsync. Deltas contain appended events, added/acknowledged pending
operations, and changed metadata. A transaction becomes visible only after its
record is durable. Recovery truncates an incomplete final record; a checksum
mismatch or revision gap fails explicitly. Backups require the baseline and
journal together under the document lock or with all writers stopped. Existing
old clients must not write the baseline after migration. The CLI `log` command
is the supported way to read the full reconstructed state.

The Android UI maintains an optimistic scene for immediate pen feedback and
submits ordered persistence jobs to a worker. A saving indicator distinguishes
queued edits from durable edits; errors remain visible. The worker publishes
materialized snapshots, not history for UI replay. Current strokes are drawn
over cached completed ink; cache invalidation preserves layer order, erasure,
page changes and pan/zoom. At most three visible layer bitmaps are cached.
JSON numbers are parsed without compiling a regex for every coordinate.

`ReplicaService` starts a loopback-only command proxy on port 8766 and performs
background synchronization for configured documents. The app calls the shared
Store in-process; its spawned CLI and external agents call `POST /command` with
the same document/action semantics. A private `proxy.json` descriptor contains
the URL and bearer token. `--proxy-file` selects it in the CLI; input files are
read by the client and sent as structured JSON. Export renders the returned
model in the client, so output files belong to the caller. The JVM CLI can run
the same service with `serve [PORT [SERVER_URL]]`.

No AI orchestration is introduced. The primary server remains the existing
desktop Python/SQLite service; the tablet's URL is configured in preferences,
not compiled into the APK. An offline proxy continues to accept local edits.

## Consequences and limits

The Android service runs while Android permits the application to run. This is
not a foreground service or a promise of continuous execution after force-stop,
background process eviction or reboot. Reopening the app restores the service
and durable outbox. An edit that is still marked Saving can be lost on abrupt
process death; completed journal transactions survive restart. Acknowledged
proxy actions are durable before their response is sent. Lost responses require
inspection before retrying mutations, as in ADR 0003.

The journal retains audit history and currently has no compaction. Startup time
therefore grows with history, and pending-index/model-index copying still grows
with document size. This change targets interactive latency through at least
1,000 strokes, not unlimited document size. Sync catch-up may reapply pending
operations to the cached committed model, preserving existing conflict semantics.
HTTP is local and authenticated with a token; use ADB forwarding for host access
to the tablet rather than exposing its editing endpoint on the LAN.

## Verification

Run `tests/cli_checks.py`, `tests/proxy_checks.py`, `tests/cli_android.py`,
`tests/integration.py`, and `tests/performance.py` through `nix-shell` after
building the CLI and APK. The performance check uses an isolated API 36 emulator,
1,000 pressure-bearing strokes of 50 points, real view dispatch and window frame
timings. It compares early, middle and late windows, bounds save backlog and
drain time, checks the incremental model against a complete replay, and reads
all 1,000 strokes from a fresh process. Measured results belong in
`docs/verification.md`; emulator timing does not measure E Ink scanout or pen
hardware latency.
