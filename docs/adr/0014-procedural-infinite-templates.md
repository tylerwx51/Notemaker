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
document origin. Increase spacing at low zoom so dots remain around five screen
pixels apart and the visible dot count stays near 3,000. Keep fixed and paged
template SVG behavior unchanged.

## Consequences

Panning and zooming require only a viewport redraw; the document does not gain
per-mark objects or a canvas-sized bitmap. The current browser viewer and
non-Android export renderers do not draw the internal infinite-template object;
they remain read-only/general-purpose renderers and need their own bounded
viewport implementation if infinite template previews are required there.

## Verification

The Android and Android CLI builds succeed. A CLI operation on an infinite
document stores one compact template object. E Ink refresh and stylus behavior
still require validation on BOOX hardware.
