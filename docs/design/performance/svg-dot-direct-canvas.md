# Direct Android dot-grid drawing — NoteAir6C, 2026-10-21

The checkpointed copied `notes` fixture contains a 122,125-character SVG with
1,333 circles. In the JSON-only baseline, the first cold draw spent roughly
35 ms validating this generated grid, 102 ms parsing it with AndroidSVG, and
59 ms recording the resulting Picture. It was the largest measured first-draw
SVG cost.

This branch keeps the document's SVG and all CLI/server protocols intact. The
strict shared dot-grid recognizer returns parsed circles for its exact grammar.
Android records those circles directly into its existing 64-entry Picture
cache, using the four-cubic path construction from AndroidSVG. Other SVGs keep
the established renderer and strict validation. A first grid recording takes
about 46 ms on the physical NoteAir6C; subsequent tile uses replay the cached
Picture.

| Ten process-cold launches after two warmups | JSON-only baseline | Direct grid, run 1 | Direct grid, run 2 | Final source, run 3 |
| --- | ---: | ---: | ---: | ---: |
| Android total median | 1,428.5 ms | 1,339.5 ms | 1,338 ms | 1,323 ms |
| Process CPU median | 2,555 ms | 2,430 ms | 2,430 ms | 2,425 ms |
| Sampled PSS median | 157.1 MiB | 153.5 MiB | 153.4 MiB | 153.6 MiB |
| Logical process reads median | 1.89 MB | 1.89 MB | 1.89 MB | 1.89 MB |

The first attempt drew every tile separately and reached only 1,409.5 ms
median. Reusing a Picture for the direct path made the difference. The SVG
source and checkpoint bytes are unchanged, so this is not an I/O optimization.

Android coverage compares the direct Picture against the ordinary SVG renderer
on a 1,333-circle grid at 240×240 and 1000×1400 output sizes, plus a
translucent colored variant. The paths align,
but edge rasterization is not bit-for-bit identical. At 240×240, 836 of 57,600
pixels differed; the maximum alpha difference was 7/255. The instrumented test
bounds affected pixels, alpha error, and premultiplied color error at both
sizes. The exact-pixel check for ordinary SVG Picture reuse is retained.

The physical runs used an isolated package and copied note. Raw results are
private under `.test-artifacts/hillclimb/tablet-svg-dot-direct-picture-{1,2,3}/`;
the discarded per-tile attempt is under `tablet-svg-dot-direct-1/`. The full
Android drawing suite, 22 CLI checks, and 17 proxy checks passed. Physical
E Ink scanout was not measured.
