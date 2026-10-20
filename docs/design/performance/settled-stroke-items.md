# Reuse settled-stroke tile items — NoteAir6C, 2026-10-20

Branch `perf/settled-stroke-items` retains each settled stroke's tile item while its immutable model map remains the same object. A replacement stroke map rebuilds the item, and document changes, detachment, and removed strokes clear the corresponding cache entries. This avoids recomputing bounds and allocating a wrapper for every unchanged settled stroke on every canvas draw. Rendering order, tile membership, and persisted data are unchanged.

| Physical 1,000-stroke run | Prior branch 1 | Prior branch 2 | Cached 1 | Cached 2 |
| --- | ---: | ---: | ---: | ---: |
| Overall input-to-app-draw p95 | 68.0 ms | 67.5 ms | 61.0 ms | 60.9 ms |
| Strokes 901–1,000 input-to-draw p95 | 52.0 ms | 49.6 ms | 32.3 ms | 32.2 ms |
| Strokes 901–1,000 frame draw p95 | 31.2 ms | 29.6 ms | 13.1 ms | 13.1 ms |
| Strokes 901–1,000 full frame p95 | 91.0 ms | 91.0 ms | 91.3 ms | 91.1 ms |
| Process CPU delta | 290.6 s | 291.8 s | 281.5 s | 279.6 s |
| Charged write-byte delta | 36.8 MiB | 36.6 MiB | 36.7 MiB | 36.4 MiB |
| Peak sampled PSS | 265 MiB | 279 MiB | 259 MiB | 243 MiB |

The app-draw gain repeated, especially late in the dense document. Window-frame p95 did not improve: its command-issue phase remained near 50 ms. Charged writes stayed close to baseline, as expected for a render-only change. PSS snapshots vary across runs, so they do not establish a memory saving. All four runs passed incremental/full replay parity and reopened with 1,000 durable strokes. Note-free measurements and APK hashes are in [settled-stroke-items.json](settled-stroke-items.json).

The emulator live, pen-up, persisted, rebuilt, mask, coverage, viewport, seam, pan/zoom, and cache checks passed. Its strict 1,000-stroke gate kept every measured input under 30 ms (p95 17.19 ms, maximum 22.27 ms) and verified replay and restart state. These are Android app and window timings from synthetic input; they do not measure physical stylus sampling or E Ink panel scanout.
