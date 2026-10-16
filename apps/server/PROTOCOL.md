# NoteMaker operation protocol (v1)

All clients use HTTP JSON. A document name is a case-sensitive identifier.
Coordinates are logical units rather than screen pixels. Fixed pages are
1,000 × 1,400; infinite documents use the same units without clipping.

`POST /api/events` takes:

```json
{"document":"notes","operations":[{"id":"unique-operation-id","kind":"document","type":"fixed"}]}
```

It returns `{"accepted":["unique-operation-id"]}`. A batch is committed as one
SQLite transaction. An identical retry is acknowledged without duplicating its
log record. Reusing an operation ID with different contents returns HTTP 400.

`GET /api/events?document=notes&after=0` returns:

```json
{"events":[{"seq":1,"op":{"id":"unique-operation-id","kind":"document","type":"fixed"}}]}
```

Sequence numbers are global, strictly increasing, and may have gaps within a
document. Reads return at most 1,000 events; continue with the last received
sequence until a read returns fewer than 1,000. The legacy browser polls at 200 ms.
Durable proxies use the version 3 protocol below; they do not poll documents while idle.

Each operation requires an immutable `id`. Supported kinds:

| Kind | Other fields | Replay behavior |
| --- | --- | --- |
| `document` | `type`: `fixed`, `pages`, or `infinite` | First document operation sets the type; later creation attempts do not change it. |
| `layer` | `layer`: ID, `name`: string, `force`: boolean | Create/update a layer; creation order determines compositing order. |
| `stroke` | `stroke`: ID, `layer`: ID, `page`: zero-based integer, `color`: signed ARGB integer, `width`: number, `points`: `[[x,y,pressure],...]` | Replace that stroke's previous snapshot. |
| `active` | `stroke`: ID, `active`: boolean | Show/hide the stroke, used for erasure and undo/redo. |

The implicit bottom layer has ID `base`, name `Layer 1`, and `force: false`.
Visibility choices belong to each client. A layer with `force: true` is visible
regardless of those choices. Releasing force restores local visibility.

Pressure is clamped to 0–1 on capture. Render each point as a circle of radius
`width * (0.25 + 0.75 * pressure) / 2`. Connect adjacent points with a round-capped
line whose width is the average of the adjacent point widths. Layer compositing
order precedes stroke creation order. Fixed documents and infinite canvases use
page 0; multipage documents filter by their current page.

Snapshot and active updates resolve in server sequence order. Stroke IDs are
client-generated UUIDs, so independent clients' new strokes merge. Layer edits,
erasure, and undo are shared operations; local visibility is never uploaded.

Limits: 200 operations and 4,000,000 request bytes per batch; 10,000 points per
stroke snapshot; widths 1–100; pages 0–10,000. The shared replica batches at most
100 operations and approximately 3 MB JSON. Android reduces long strokes at 8,000
points. This is an append-only audit log; no compaction endpoint is provided.

## Version 2: local replicas and diagram actions

Upgrade server, app, and CLI together before using v2 operations. Existing v1
operations and logs remain valid; old clients cannot render v2 batches/objects.
`GET /api/info` returns `{id: <persisted database UUID>, protocol: 2}`. Clients pin
this identity after synchronization; an unrelated database must use another replica.

New operation kinds:

| Kind | Fields | Replay |
| --- | --- | --- |
| `batch` | `actions`: 1–200 non-batch operations; optional `actor` | One atomic action and one sequence entry. Replay actions in array order. Child IDs are not required. |
| `object` | `object`, `shape`, `layer`, `page`, `x`, `y`, `w`, `h`, `color`, `width`; optional `fill`, `text`, `fontSize`, `from`, `to` | Replace the object's geometry/style snapshot; retain insertion order. |
| `object-active` | `object`, `active` | Last-operation-wins object erasure/restoration, independent of geometry. |
| `layer-active` | `layer`, `active` | Shared layer erasure/restoration. Base layer cannot be deactivated. |

`shape` includes `rect`, `ellipse`, `text`, `arrow`, `line`, `triangle`, `diamond`,
`pentagon`, `hexagon`, `star`, `svg`, and `template`. SVG objects require an
additional `svg` string containing a self-contained static SVG (up to 256 KiB).
The `x,y,w,h` rectangle is its clipped viewport; the SVG's `preserveAspectRatio`
controls fitting, defaulting to centered meet. Source is embedded in every
snapshot, including undo history; no asset URL or separate upload is involved.
See [the import profile](../cli/README.md#place-a-generated-svg) for supported
elements. The server validates that profile before accepting a batch. SVGs use
the same layer/page, active-state, ordering, and actor history as other objects.

Dimensions are positive except for lines/arrows, which use `(x,y)` and
`(x+w,y+h)` as endpoints (fallback endpoints for attached arrows). Optional `from`
and `to` reference node centers, with endpoints intersected at node boundaries;
connectors follow current node geometry. `color`/`fill` are signed 32-bit ARGB;
`width` is 1–100, `fontSize` 1–500, `text` at most 10,000 characters. Geometry is
finite and within ±1,000,000 units. Page limits are unchanged.

The internal `template` shape carries infinite-canvas `template` (`ruled`,
`grid`, or `dots`) and `spacing` fields. It is stored in the ordinary object
operation and travels through event replication and proxy trees with the rest of
the document. Clients render the pattern against their current viewport; the
template remains one compact object regardless of canvas size.

Native polygons use normalized catalog vertices mapped to the object's bounds,
round outline joins, and optional ARGB fill. Their persisted representation is
an editable object snapshot, not SVG source or pen strokes. CLI `shape list`
exposes the catalog through the proxy without changing document history. New
shape IDs require a coordinated upgrade of app, CLI, proxies, and main server;
the replication protocol version is unchanged. Selection locks and conflict
version retention are planned in ADR 0009 and are not yet protocol features.

Absent active registers mean true. Deactivating a layer suppresses its strokes and
objects even if force-visible. Layers composite in creation order; within a layer,
strokes precede objects, each in creation order. Undo creation leaves tombstoned
entities in the model and log. Object snapshots do not implicitly reactivate them.

A batch must validate in full before any member is accepted or rendered. Batch IDs
have the same idempotency and collision rules as all other operation IDs. Optional
`actor` records editing provenance; it is not authentication. The server commits
operations in arrival order. Independent geometry and visibility registers mean a
concurrent move and erase merge to a moved but hidden object. Same-register edits
resolve to the last committed operation. Local pending actions are replayed after
committed events until acknowledgement and full catch-up.

## Version 3: durable proxy trees

Upgrade server, proxies, CLI and app together. Legacy event endpoints remain
available to the browser. The root database and operation payloads are unchanged.

`GET /api/info` returns `protocol:3`, stable `id`, `kind:"server"|"proxy"`,
`rootId` (null until known), locally opened `documents`, and proxy `lineage`.
A server's rootId equals its id. Proxy requests require the descriptor's bearer
token, including SSE. All proxy listeners remain loopback-only.

A proxy has one upstream. Configure it using the proxy command
`configure URL_OR_DESCRIPTOR`. The descriptor path is on the proxy host.
An empty string disconnects. `GET /api/info?trail=ID,ID&validate=1` validates
a proposed proxy edge by walking proxy ancestors, rejecting repeated stable
IDs or more than 64 hops. Existing edges do not require reachable ancestors for
ordinary reads or receipt of actions. A main-server identity is pinned for the
replica; switching to a different database is rejected before upload.

`POST /api/events` retains its request and response shape. At an intermediate
proxy, an accepted ID means the action was durably received. Only the root
assigns final commit order. Duplicate identical actions are acknowledged without
reapplication; reuse of an ID with different contents fails atomically. A sender
retains an action until it downloads the root commit, even after durable receipt
by its parent. Forwarded receipt IDs avoid redundant uploads.

Protocol 3 senders also include their known `rootId` in the upload envelope.
An unbound receiving proxy pins that identity durably; an already bound proxy
or server rejects a different identity before accepting actions. This prevents
mixing server histories through a temporarily disconnected, unbound parent.

`GET /api/replica?document=notes&after=0&pendingAfter=0` returns:

```json
{
  "rootId": "main-server-database-id",
  "events": [],
  "after": 0,
  "more": false,
  "provisional": [],
  "pendingAfter": 0,
  "pendingMore": false
}
```

`events` contains main-server-ordered operations. `after` is a **per-document
operation count**, not the legacy SQLite global sequence. `provisional` is a
page of the parent proxy's append-only ingress log; it may include operations
already committed. Deduplicate by immutable operation ID. Its cursor belongs
to that upstream proxy ID and resets if the upstream changes. At the main server,
the provisional stream is empty and pendingAfter is zero. Each stream page
contains at most 100 actions and approximately 3 MB; continue while either
more flag is true. Apply whole actions and advance cursors atomically. Never
treat a provisional receipt/order as a main-server commit.

`GET /api/stream` keeps an SSE response open with `Content-Type: text/event-stream`.
A `data: <generation>` event is a wake-up notification; it is not durable history.
Each new connection immediately sends an event, followed by events on changes.
Idle connections receive `: heartbeat` every 15 seconds. Clients reconnect with
backoff and read from their **durable replication cursors**, including after a
missed notification or peer restart. Generation values are process-local;
Last-Event-ID is deliberately unnecessary for recovery.

One worker per proxy handles upstream synchronization. The first pending local
action starts a 500 ms upload timer; subsequent actions do not reset it. Upload
starts are at least 500 ms apart across all documents, with only one in flight.
Batches preserve whole actions and are capped at 100 actions and approximately
3 MB of action JSON. Failure backoff grows from 500 ms to 30 seconds. Notifications
can trigger downloads immediately without bypassing upload limits.

The local command endpoint also accepts `snapshot` (materialized effectiveModel,
revision, cursor, eventCount, pendingCount and editor preferences), `append -`
(one raw editor action), and `settings -` (local hidden/undo/redo arrays, selected
layer and page). Preferences do not replicate. Direct file readers are supported;
direct file writers and standalone CLI sync workers are not.

## Selection coordination and causal drawing edits

The transient `/api/locks?document=NAME` endpoint returns `leases` and `previews`.
Proxies forward acquisition, renewal, release and preview requests through
`POST /api/locks`. Entity keys include page, kind and ID. Leases identify an
actor and session and expire after 15 seconds without renewal. A reachable
authority accepts only one session per entity; the caller receives accepted and
skipped entities. Clean disconnect releases the actor's selections. Delayed
lease deliveries must not resurrect a released selection. Disconnected clients
may hold local leases; reconciliation preserves conflicting offline versions.

Previews are transient and do not become drawing operations. Consumers discard
them on cancellation or lease expiry and render the last committed content.
A completed transform replaces the affected entity snapshots in one batch.
Locked entity dependencies also guard undo, bulk operations and inbound edits.

New drawing actions have a `bases` object mapping `object:ID`/`stroke:ID` to the
version on which the edit was based, plus `visibility` for that branch's active
states. Empty base strings represent new entities. Existing actions without
these fields retain legacy replay behavior. Deterministic replay preserves both
snapshots when an action diverges from the current version, including same-actor
offline edits. Later edits on a branch update its retained snapshot. A
`conflict-status` operation has `{kind:"conflict-status",conflict:ID,active:false}`
and participates in the same atomic batch as the chosen replacement snapshots;
its inverse restores the conflict. Replicas do not upload derived conflict lists.

Drawing extensions include text style fields, brush identifiers, embedded PNG
and JPEG objects, optional object rotation/reflection and layer order. The same
validation applies locally and at the main server. Upgrade all applications
together before using the new operations.
# Draw-item erasure masks

Object and stroke snapshots optionally carry `erasures`: an array of polygon
contours, each an array of finite `[x,y]` pairs. Each contour has 3–4096 vertices;
there are at most 2000 contours and 64000 total vertices, with coordinate
magnitude at most 1e9. Existing atomic action/transport size limits also apply.
The effective painted region subtracts the union of contours, using even/odd
fill within each contour. This is clipping, never painting the page background.

For objects, vertices are relative to the unrotated item's origin and normalized
by its width/height (zero dimensions use one). The object's normal transform
applies to content and mask together. Attached connectors use resolved endpoint
geometry. Stroke vertices are offsets from the first point, in document units;
stroke transforms update those vectors along with the source points.

Masks are optional fields on existing snapshots, so history, causal versions,
conflicts and batching retain their current format. The durable transport remains
version 3. All renderers/proxies must be upgraded before using masks: old clients
can ignore unknown fields and display erased source content. No remote rollout
is performed by the local implementation change.

## Document deletion

`{"kind":"document-delete","id":"unique-operation-id","actor":"human"}`
is a standalone lifecycle operation carried by the existing events/replica
protocol. Nested deletion in a batch is rejected. App and CLI callers submit it
to their selected local proxy, which journals and forwards it normally.

Deletion is permanent and wins over every creation/content operation, regardless
of order. Replay exposes `deleted:true`, clears the effective drawing, and ignores
subsequent content operations. Already queued stale operations are still accepted
and acknowledged so offline replicas can drain their queues and converge. New
local edits and undo/redo against a known tombstone are rejected. Operation logs,
cursors, identities and actor histories remain intact; names cannot be reused.

The server stores a transactional `deleted_documents` index alongside its events.
`GET /api/info` excludes these names from `documents` and returns them separately
in `deletedDocuments`. Replication and audit endpoints remain readable by name
and continue to return the complete log. Server agent document choices exclude
deleted names. No HTTP DELETE or physical purge is provided. Upgrade all peers
before using this new operation kind.
