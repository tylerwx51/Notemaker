# Direct proxy snapshot for object creation — NoteAir6C, 2026-10-20

`object put snapshot -` now commits the object action in the shared local Store
and builds the response snapshot from that same durable edit in the proxy. The
generic proxy route read the Store before and after the CLI call, even though
the action response already contained the post-edit state. The direct route
avoids those reads and still notifies other clients of the change. The Android
editor uses the returned snapshot for its first redraw after creation.

Two isolated physical NoteAir6C runs used the same copied `notes` document and
40-rectangle sweep as the parent. Five actions warmed each run. All 40 objects
and their actor-history entries were verified in the durable replica. CLI,
proxy, and drawing-tool checks passed with actual CLI calls; the Android build
passed.

| Measure | No-pending-frame parent | Direct snapshot |
| --- | ---: | ---: |
| Input to updated window draw p95 | 62.99 / 60.74 ms | 58.83 / 56.79 ms |
| Input to updated Canvas draw p95 | 86.47 / 79.27 ms | 76.12 / 79.35 ms |
| Measured window frames | 43 / 43 | 44 / 42 |
| Charged writes | 748 / 744 KiB | 748 / 744 KiB |
| Process CPU | 3.70 / 3.63 s | 3.36 / 3.60 s |
| Sampled PSS after sweep | 235 / 235 MiB | 231 / 231 MiB |

The two window p95 results improve by about 4–6 ms. Canvas p95, frames,
CPU, and writes do not show a repeatable change; sampled PSS is 4 MiB lower.
These synthetic input timings end at Android app/window draw, before physical
E Ink scanout. Raw metrics and APK hashes are under
`.test-artifacts/hillclimb/tablet-object-create-direct-snapshot-{1,2}/`.
