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
sequence until a read returns fewer than 1,000. Clients poll at 200 ms. Do not
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
stroke snapshot; widths 1–100; pages 0–10,000. Android batches at most 100
operations and approximately 1 MB JSON, and reduces very long strokes at 8,000
points. This is an append-only audit log; no compaction endpoint is provided.
