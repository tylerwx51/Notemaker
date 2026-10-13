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
sequence until a read returns fewer than 1,000. The browser polls at 200 ms;
the local proxy waits 500 ms between synchronization passes. Do not
remove acknowledged optimistic operations until catch-up has completed.

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

`shape` is `rect`, `ellipse`, `text`, `arrow`, or `svg`. SVG objects require an
additional `svg` string containing a self-contained static SVG (up to 256 KiB).
The `x,y,w,h` rectangle is its clipped viewport; the SVG's `preserveAspectRatio`
controls fitting, defaulting to centered meet. Source is embedded in every
snapshot, including undo history; no asset URL or separate upload is involved.
See [the import profile](../cli/README.md#place-a-generated-svg) for supported
elements. The server validates that profile before accepting a batch. SVGs use
the same layer/page, active-state, ordering, and actor history as other objects.

Rect/ellipse/text/SVG dimensions are
positive; arrows use `(x,y)` and `(x+w,y+h)` as fallback endpoints. Optional `from`
and `to` reference node centers, with endpoints intersected at node boundaries;
connectors follow current node geometry. `color`/`fill` are signed 32-bit ARGB;
`width` is 1–100, `fontSize` 1–500, `text` at most 10,000 characters. Geometry is
finite and within ±1,000,000 units. Page limits are unchanged.

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
