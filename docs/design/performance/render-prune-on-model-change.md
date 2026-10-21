# Prune Android render items on model changes — NoteAir6C, 2026-10-21

The editor previously intersected its settled stroke and object item-cache
keys with all model keys inside every layer's `settledInk()` call, including
live-ink frames with no model change. The renderer now prunes once on the first
draw after `refreshShared()` or an optimistic action. Document, type, and page
changes still clear their existing render caches. This changes neither item
order nor persistence.

Two candidate runs and a fresh parent run used the same isolated 1,000-stroke
workload on the physical NoteAir6C. The older parent run is included for
context. Each run includes 10 warmup strokes and verifies incremental/full
replay parity and all 1,000 strokes after reopening.

| Measurement | Parent, earlier | Candidate 1 | Candidate 2 | Parent, fresh |
| --- | ---: | ---: | ---: | ---: |
| Overall input-to-app-draw p50 | 38.70 ms | 37.82 ms | 36.46 ms | 38.92 ms |
| Overall input-to-app-draw p95 | 61.74 ms | 61.13 ms | 61.21 ms | 61.68 ms |
| Last 100 strokes, draw phase p95 | 12.45 ms | 12.81 ms | 12.27 ms | 11.94 ms |
| Last 100 strokes, full frame p95 | 91.60 ms | 91.58 ms | 91.37 ms | 91.49 ms |
| Process CPU, stroke 0 to 1,000 | 285.9 s | 282.9 s | 282.5 s | 286.9 s |
| Charged writes, stroke 0 to 1,000 | 38.67 MB | 38.48 MB | 38.46 MB | 38.66 MB |
| PSS after explicit GC | 187.9 MiB | 187.2 MiB | 186.9 MiB | 186.3 MiB |

The A/B/A comparison supports a modest CPU reduction (about 1–1.5%) and a
small overall app-draw latency improvement. Late draw and full-frame p95 did
not improve consistently, and sampled memory varies with GC. The code is
small and the saving is repeatable, so this branch becomes the preferred
render path. No disk format or I/O behavior changed; the small write-counter
variation is within normal run variation.

The rebuilt candidate APK passed the full Android drawing suite, including
live/settled/rebuilt pixels, selection and layer changes, masks, viewport
cache behavior, and restart. Raw private measurements are under
`.test-artifacts/hillclimb/tablet-render-prune-generation-strokes-{1,2}/`
and `tablet-render-prune-parent-strokes-1/`; the older parent run is under
`tablet-layer-cache-resource-1000-1/`. Timings end at Android drawing or
frame submission and exclude physical stylus sampling and E Ink scanout.
