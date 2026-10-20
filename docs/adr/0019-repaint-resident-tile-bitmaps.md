# ADR 0019: Repaint resident tile bitmaps in place

Status: accepted on `perf/reuse-tile-bitmaps`, 2026-10-20.

## Context

ADR 0017 capped live tile pixels but replaced a changed tile's bitmap on every
redraw. A 1,000-stroke physical tablet run rebuilt about 8,100 tiles and
sampled a 228 MiB Android graphics PSS peak, despite about 20 MiB of live
tile pixels. Per-edit allocation and recycling could contribute to graphics
memory held outside the cache's own byte counter.

## Decision

When a tile key remains resident and its contributors change, clear its mutable
ARGB bitmap to transparent and repaint the full tile using the same transform,
gutter clip, item order, and Skia calls. Retain its shader and cache byte count.
Allocate a bitmap only for a new key or after LRU eviction. Empty tiles still
leave the cache and recycle their bitmap. This retains the exact full-repaint
contract; it does not use partial damage repaint or change the tile budget.

## Evidence and limits

Emulator pixel contracts passed for live strokes, pen-up, masks, item changes,
tile seams and viewport culling. The strict 1,000-stroke emulator gate passed
all 7,920 inputs under 30 ms and verified durable restart. Two isolated runs
on the physical NoteAir6C reduced peak sampled process PSS from 404 MiB to
292 and 278 MiB, and graphics PSS from 228 MiB to 88 and 89 MiB. The tablet's
app draw and window frame p95 did not meaningfully change; this decision
addresses memory, not the outstanding latency target. Details and sanitized
counters are in `docs/design/performance/bitmap-reuse-experiment.md`.

Android's graphics PSS is process accounting, not an exact per-bitmap inventory.
The physical runs recorded app frame timings but did not independently measure
E Ink scanout or compare every displayed pixel on BOOX firmware. The software
pixel checks cover the drawing contract exercised by the repository tests.
