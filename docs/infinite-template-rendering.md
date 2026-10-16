# Infinite canvas template rendering

The dot, grid, and ruled guides on an infinite canvas are stored as one small
template object. The Android renderer generates only the marks intersecting the
current viewport; panning or zooming recomputes those marks from document
coordinates. A density floor limits dots and lines at far zoom levels.

```mermaid
flowchart LR
  A[Template selection] --> B[CLI shared template action]
  B --> C[One compact template object\nkind, spacing, color, page]
  C --> D[Replica and local persistence]
  D --> E[Android viewport render]
  E --> F[Convert visible screen bounds\nto document coordinates]
  F --> G[Align first mark to global spacing origin]
  G --> H[Generate visible rows, columns, or dots]
  H --> I[Draw clipped to viewport]
  J[Pan or zoom] --> E
```

The renderer avoids a canvas-sized bitmap and avoids storing per-mark geometry.
The visible dot count is capped at roughly 3,000 marks, and grid/rule spacing
also stays at least five screen pixels apart. Fixed and paged templates continue
to use their existing SVG representation.
