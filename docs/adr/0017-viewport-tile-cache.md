# ADR 0017: Document-space viewport tile cache

Status: accepted, 2026-10-19.

## Context

The Android canvas previously kept one display-sized bitmap per layer and
included pan offsets and zoom in its cache key. A small viewport movement could
therefore recycle every settled layer bitmap. Repainting also traversed all
settled content, and image objects decoded their embedded PNG on each repaint.

## Decision

Settled Android ink is cached in document-space tiles with a two-raster-pixel
gutter. Tiles cover 256 document units at raster tiers 1 and 2, 128 units at
tier 4, and 64 units at tier 8. The main canvas draws those tiles under its
current document transform. Panning reuses tile pixels; zoom selects the
nearest power-of-two raster density, so a tile is refined only when the
effective display scale crosses a density tier. Tiles are shared across
visible layers and density tiers in a 96 MiB per-canvas LRU budget.
Changed, added, removed, or reordered content rebuilds only tiles touched by the
edit. A changed tile replays all of its intersecting contributors; partial-tile
damage repaint changed Skia anti-aliasing at clipped item edges and failed the
exact live/committed pixel contract. Conservative item paint bounds cull items
before they reach the shared renderer. A gutter supplies edge samples while
each tile composites only its core, avoiding double-compositing translucent ink
at seams.

Settled strokes retain bounded native Path draw commands compiled through the
same stroke renderer and Android Png polygon/overlap operations. Repainting a
changed tile replays those paths with the original Canvas.drawPath calls,
avoiding repeated stroke resampling and path construction. Live strokes use a
per-frame command cache so a stroke crossing several tiles is compiled once
per frame. The command cache is limited by entry count and estimated path-point
and byte weights; these weights bound admission but are not exact native heap
measurements. Strokes with changed source maps regenerate their commands.

Android image objects use a separate decoded-bitmap LRU capped at 24 MiB, with
embedded-data keys capped at 4 MiB total. Oversized images or keys are drawn and
released without entering the cache. These caches affect presentation only;
layer order, action state,
offline behavior, fixed-page clipping, and the live/committed rendering
contract in ADR 0016 remain in force.

## Consequences

Pan and zoom within a density tier reuse settled pixels. Density tiers limit
rebuilds during gestures while keeping raster resolution proportional to the
effective display scale. Bitmap filtering can soften edges slightly between
tiers, and crossing a tier rerenders visible tiles once. Both tile and decoded
image memory have explicit bounds. Rendering still checks current item bounds
to account for document edits, while only tile contributors are sent to the
renderer. Performance instrumentation reports tile rerender/reuse and
item-render counters alongside input latency; it does not use the retired
layer-bitmap rebuild counter as evidence of tile reuse. Performance metrics
also report native path-command reuse and estimated cache bounds.

## Limits

The Android `--ink-only` pixel contract and `--viewport-only` checks passed on
an API 36 emulator, including fixed/infinite page edges, tile seams, object
invalidation, pan reuse, zoom-tier changes, erasure, and the memory bound. The
isolated 1,000-stroke performance suite also passed: all 7,920 input events
stayed under 30 ms, with p95 17.38 ms and maximum 22.54 ms. The run recorded
285,856 native path-command reuses, 9,000 geometry renders, and an estimated
5.67 MiB of command data under the 24 MiB admission budget. Replay and restart
checks passed against an uncullled same-tier reference with exact pixel
equality. Software canvas measurements do not establish BOOX E Ink refresh
latency or stylus behavior; those still require hardware validation.
