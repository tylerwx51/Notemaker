# ADR 0014: Procedural infinite canvas templates

Status: accepted, 2026-10-16.

## Context

Infinite canvases have no finite page bounds, so serializing each dot or rule as
SVG geometry grows with the configured page dimensions and cannot cover the
whole canvas. Rendering marks outside the viewport also wastes memory and CPU.

## Decision

For infinite documents, store ruled, grid, and dot backgrounds as one shared
template object containing its pattern, spacing, and color. Generate marks in
Android from the current viewport in document coordinates, aligned to the
document origin. Use a medium-dark default, 1.4 screen-pixel rule/grid lines,
and 4.2 screen-pixel dot diameters. Increase spacing at low zoom so dots remain
around five screen pixels apart and the visible dot count stays near 3,000.
Render legacy light-gray infinite templates using the darker default. Keep fixed
and paged template SVG behavior unchanged. The main server validates and
replicates the compact template object like other object snapshots. Network
viewers draw it within their current infinite-canvas viewport.

For fixed and paged dot-grid templates, recognize the exact SVG grammar emitted
by the template action during SVG validation. Check every circle and the
complete document, then normalize its viewport without building an XML DOM.
All other SVG input keeps the general validator. This changes validation cost,
not the stored template action.

Android also renders that exact generated dot-grid grammar directly as a cached
Canvas Picture. The shared validator supplies bounded, parsed circle geometry;
Android records the same four-cubic circle paths used by AndroidSVG. Other SVG
assets still use AndroidSVG. The source, CLI export, local journal, and network
protocol remain unchanged. The direct picture has small edge antialiasing
differences from AndroidSVG, bounded in instrumented comparisons at both tile
and full-page sizes.

## Consequences

Panning and zooming require only a viewport redraw; the document does not gain
per-mark objects or a canvas-sized bitmap. Non-Android export renderers do not
draw the internal infinite-template object; they need their own bounded viewport
implementation if infinite template exports are required there.

## Verification

The Android and Android CLI builds succeed. A CLI operation on an infinite
document stores one compact template object. Server validation and proxy
replication preserve its `template` and `spacing` fields. E Ink refresh and
stylus behavior still require validation on BOOX hardware.
On a copied NoteAir6C note with a 1,333-circle fixed-page template, this
validation fast path reduced process-cold launch median from 1,614 ms to
1,466/1,473 ms in two runs. Android pixel comparison against the general DOM
path matched exactly for a 1,333-circle template. See
`docs/design/performance/svg-dot-grid-fast-validation.md`.
On the later checkpointed copied note, direct Canvas rendering with picture
reuse reduced process-cold launch median from 1,428.5 ms to 1,339.5, 1,338,
and 1,323 ms in three physical runs, and first rendering of the 122 KiB grid from
about 196 ms of validation, SVG parsing, and picture recording to about 46 ms.
See `docs/design/performance/svg-dot-direct-canvas.md`.

## Update 2026-11-02

The Java implementation described here was ported one-to-one to Rust and
removed (ADR 0026 CLI, ADR 0029 Android app, `docs/design/rust-port.md`).
The decision stands; JVM, `app_process`, jsvg/androidsvg and `.java` details
now refer to their Rust equivalents (native CLI binary, resvg).
