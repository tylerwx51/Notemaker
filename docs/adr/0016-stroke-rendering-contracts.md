# ADR 0016: Crossing ink and stable stroke completion

Status: accepted, 2026-10-19.

## Context

A single even-odd ribbon left holes at self-crossings and could not reproduce
translucent ink deposited by two crossing strokes. Android also drew live ink
on top of settled layer pixels and objects, then moved it into a transparent
layer bitmap below objects at pen-up. This changed both stacking and antialias
compositing when a stroke finished.

## Decision

The shared renderer splits ribbons into passes as the centerline turns. Raster
surfaces merge shared pass edges before painting; overlap receives additional
pigment, preserving translucent crossings without seams at artificial splits.
Opaque ink uses a nonzero compound path. SVG exports express the same coverage
with a compound path and clipped additional passes. Coverage consumers continue
to use the union of the shared paint primitives. Object polygon fill rules and
erasure masks retain their existing semantics.

Android renders the current stroke through the same layer order as settled
ink. The document-space tile cache rebuilds only tiles touched by the extending
stroke, replaying all intersecting items in each changed tile to preserve exact
anti-aliasing; pan reuses unchanged tile pixels. Stroke bounds use conservative
paint extents to avoid false culling. Viewport cache behavior is recorded in
ADR 0017.
Committing or rebuilding a stroke therefore does not switch its
compositing path. This is a presentation change; actions, local persistence,
actor histories and the offline protocol are unchanged.
The Android live path now builds the shared item map directly from the
in-memory point array, preserving its original numeric values instead of
serializing and reparsing the stroke on every frame. The same map feeds
emission, so live and persisted geometry follow one representation. Physical
app-draw timing and the numeric-serialization tradeoff are recorded in
`docs/design/performance/live-stroke-map-experiment.md`.

Regression coverage includes actual CLI PNG/SVG exports, Android crossing
pixels, and complete drawing-canvas pixel equality before and after live
emission, pen-up, persistence and cache reconstruction. Crossing comparisons
require exact uniform interiors and allow bounded antialias edge differences
between a compound shape and two separately rasterized strokes. Completion
comparisons allow no changed pixels. The latter cover all eleven brushes and
fractional transforms with existing translucent ink and overlapping objects.

The emulator performance suite enforces under 30 ms from asynchronous UI input
enqueue to the end of canvas drawing for every event after ten warmup strokes.
Each coalesced event retains its own start time, with separate DOWN/MOVE/UP and
500-point figure-eight measurements. Input uses asynchronous messages, matching
Android input's ability to bypass traversal synchronization barriers; synchronous
instrumentation messages otherwise add an artificial extra frame of waiting.
Save-queue, growth, frame and durable-restart checks remain separate gates.

## Limits

Software canvas completion is not GPU presentation or physical pen-to-E Ink
latency. Physical refresh and stylus behavior still require BOOX hardware.
Performance runs must be isolated from other emulator/build workloads. The
drawing workload uses an unreachable sync endpoint. Only the disposable
performance emulator has Wi-Fi/mobile radios, Google Search and Play services disabled to
suppress unrelated first-boot activity; this does not airgap emulator Ethernet
or change other devices. The
pixel contract covers the drawing canvas; toolbar status can legitimately
change while a local write completes.
