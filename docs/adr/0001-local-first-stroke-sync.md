# ADR 0001: Native Android client and local stroke-log server

Status: accepted, 2026-10-09.

## Context

The first application targets Android 16 on the BOOX Note Air6 C. Finger and
stylus drawing, pressure-sensitive colors, layers, undo/redo, three document
types, and simultaneous collaboration must work on a trusted LAN. Drawing must
continue offline. The server runs on the development computer; remote deployment
is outside this change.

## Decision

Keep two owning applications in `apps/android` and `apps/server`. Use a native
Java Activity and Canvas without external application dependencies. Build the
APK with the SDK's aapt2/d8/apksigner and the JDK supplied by root `shell.nix`.
This keeps the small initial application independent of Gradle/plugin downloads.
The signing key is a generated, ignored development key, not a release key.

Use Python's standard-library HTTP server and SQLite for the local server. It
serves a browser canvas viewer and a durable, append-only operation log. No
account or login is required. Bind address, port, database, client URL, and document
name are configurable. HTTP is intentionally permitted for trusted LAN usage.

Each operation has a UUID; the server uniquely indexes `(document, operation)`.
Repeated identical operations are acknowledged without reinsertion; reuse with
different contents is rejected. SQLite transactions serialize concurrent writes.
Clients poll every 200 ms and upload batches from a locally persisted outbox.
Clients retain optimistic operations until both acknowledgement and a complete
catch-up read succeed. Both downloaded events and the outbox survive process
restart. Outbox files are atomically replaced after fsync.

Stroke snapshots use a stable stroke ID and update while drawing, at most about
10 times per second; the final snapshot is always queued at pen-up. Updates
contain document coordinates, page, layer, ARGB color, base width, and pressure
samples. Renderers draw round line segments and circles with width
`baseWidth * (0.25 + 0.75 * pressure)`. Android keeps historical input samples.
Very long strokes are reduced to every second sample at 8,000 points to bound
individual snapshot size. All snapshots remain in the server audit log.

Replay is deterministic in server sequence order. The first document operation
sets an immutable document type. Layers and strokes use last-operation wins;
concurrent layer modifications resolve in server arrival order. A stroke's
active state is a separate last-operation-wins operation. Undo affects a client's
own action history; erasing another client's stroke is an explicit shared action.
The eraser removes complete strokes on the selected layer, including line-segment
hit testing; it does not paint white over other layers.

Layer visibility is persisted separately for each Android client and held locally
in the browser. A shared `force` flag overrides local hiding until released.
Layers composite in creation order, bottom to top. Fixed and multipage documents
use 1,000 × 1,400 logical pages; multipage documents use numbered pages created on
demand. Infinite documents have unbounded coordinates, with pan and pinch zoom
available in Move mode. Move mode also supports navigation on fixed pages.

## Consequences and limits

SQLite and stdlib Python run on modest CPUs without GPU or modern vector
instruction dependencies. Polling is straightforward and reconnect-friendly;
WebSockets and operation-log compaction can follow if scale requires them. Whole
stroke snapshots and full replay favor simplicity over bandwidth and long-document
performance. The current version has no document deletion, export UI, release
packaging, or cross-server migration. Browser viewing is read-only and its local
visibility choices reset on reload.

Do not point one saved Android document at unrelated servers: its cursor and
local history refer to its original server log. Changing the URL is for reaching
the same server at a new address. Offline simultaneous edits to the same layer or
stroke resolve by server arrival order; independent strokes merge without loss.

Ordinary drawing and remote edits trigger redraws; idle polling does not redraw
the canvas. No BOOX proprietary refresh or low-latency SDK is integrated. Actual
E Ink refresh, stylus pressure calibration, palm rejection, and latency require
hardware checks. Input follows Android's [stylus and historical input
APIs](https://developer.android.com/develop/ui/views/touch-and-input/input-compatibility-on-large-screens).

## Verification

`tests/integration.py` launches two separate API 36 AVDs against the actual server,
uses instrumentation to send pressure-bearing MotionEvents to the actual canvas,
and compares replay logs and normalized rendered PNGs. It covers concurrent
strokes, duplicate upload, client-local visibility, shared force visibility,
undo/redo, erasure, offline edits, app restart, server restart, and document types.
The browser view is also served by the production server. See `docs/verification.md`
for executed checks and remaining hardware validation.
