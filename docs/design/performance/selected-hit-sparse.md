# Sparse hit test for an already selected item — NoteAir6C, 2026-10-21

Tapping an item that is already selected calls `selectedEntityAt` before
releasing its lease. The old path built a `LayerEditor` and prepared every
active item in the layer, although it only tested the few selected references.
The candidate looks up each selected stroke or object directly, checks that it
is still active and in the selected layer and page, then keeps the same exact
`DrawItem` coverage test. It does not change durable actions or lease handling.

The isolated physical NoteAir6C interaction harness now times this method
after acquiring the same selected rectangle used by its selection-tap mode.
Each run has 35 measured checks after five warmups, all required to hit the
fixture object. The copied `Drawing object protocol.` note and installed
personal app were untouched.

| Selected-item hit check | Parent | Sparse run 1 | Sparse run 2 |
| --- | ---: | ---: | ---: |
| Median | 17.20 ms | 2.18 ms | 1.77 ms |
| p95 | 23.33 ms | 2.43 ms | 2.56 ms |
| Maximum | 24.02 ms | 2.44 ms | 2.73 ms |

The selection-acquisition timing measured earlier in each cycle is not this
operation; its first updated draw stayed near the prior range. The new timing
is an in-process action measurement, not physical touch or E Ink scanout.
The full Android drawing suite passed, including selection leases, drag-box and
lasso selection, object transforms, and pixel checks. Raw private metrics and
APK hashes
are under `.test-artifacts/hillclimb/tablet-selected-hit-{baseline-1,sparse-{1,2}}/`.
