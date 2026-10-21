# Bounded cross-page tile reuse — NoteAir6C, 2026-10-20

The canvas now includes page number in each settled-ink tile key. A page switch
keeps tiles from the previous page within the existing 96 MiB LRU budget;
changing document or canvas type clears them. Each draw still compares tile
contributors with the current model, so an edit to an inactive page repaints
that page when it returns. The emulator viewport check verified both pages'
pixels, reuse on return, and invalidation after an inactive-page edit. The
full Android drawing suite also passed.

The physical NoteAir6C interaction suite alternated between two populated
pages 40 times per run, omitting five warmups. Two parent runs and three built
candidate runs used the same copied fixture. The third candidate run validated
the harness change that rebuilds the APK from current source before packaging
an isolated tablet app. Earlier exploratory captures that reused the parent
APK are excluded from this comparison.

| Page-switch measure | Parent runs | Cross-page runs |
| --- | ---: | ---: |
| Input to updated window draw p95 | 17.72 / 20.50 ms | 22.06 / 22.01 / 22.97 ms |
| Input to updated Canvas draw p95 | 27.19 / 28.59 ms | 26.07 / 26.04 / 26.96 ms |
| Window-frame p95 | 24.46 / 24.67 ms | 15.44 / 14.33 / 13.92 ms |
| Tile repaints / allocations | 80 / 80 per run | 2 / 2 per run |
| Process CPU | 1.98 / 1.97 s | 1.71 / 1.64 / 1.62 s |
| Net native allocated-heap change | +82.2 / +82.2 MiB | +2.1 / +2.1 / +2.1 MiB |
| Sampled PSS change | +41.6 / +41.7 MiB | +1.9 / +1.9 / +1.9 MiB |
| Charged writes | 328 / 324 KiB | 324 / 324 / 324 KiB |

Two pages occupy about 4.1 MiB of resident tile bitmaps rather than 2.0 MiB
for only the current page. The page-cache tradeoff is a roughly 2–5 ms later
first window draw against a roughly 10 ms faster full frame, fewer tile bitmap
allocations, and lower sampled PSS after the switching sequence. These
are process snapshots, not continuous memory high-water marks.

The candidate's isolated 1,000-stroke run kept overall input-to-app-draw p95
at 60.98 ms versus 60.83 ms on the parent, late-window p95 at 31.88 versus
31.84 ms, process CPU at 278.7 versus 277.9 s, and charged writes at 36.37
versus 36.43 MiB. All 1,000 strokes survived restart and incremental/full
replay matched. Peak sampled PSS was 245.8 versus 264.2 MiB, but one run per
branch does not establish a sustained-ink memory saving.

Timing stops at Android app/window draw before physical E Ink scanout. Raw
metrics, resource samples, and APK hashes are under
`.test-artifacts/hillclimb/tablet-cross-page-cache-current-built-{1,2,3}/`,
`.test-artifacts/hillclimb/tablet-cross-page-cache-strokes-1/`, and the parent
`tablet-tool-switch-chrome-only-{1,2}/` and `tablet-tool-switch-strokes-1/`.
