# Transient stroke bounds retention — NoteAir6C, 2026-10-21

`TileInkCache.Item` used to place every live-stroke snapshot map in a static
identity-keyed bounds cache. Each input builds a fresh map. This retained old
point arrays after the frame even though their bounds are never reused. The
preferred candidate, `perf/transient-bounds-no-cache`, computes bounds directly
for transient ink while leaving settled item caching intact.

The same isolated 1,000-stroke NoteAir6C workload ran on an unchanged build
with a measurement-only probe (`perf/bounds-cache-memory-probe`) and on the
candidate with that probe. Both runs preserved 1,000 strokes after restart and
matched incremental state to replay. The probe recorded bounds-cache occupancy
and process memory one second after an explicit GC, after timed drawing ended.

| Measure | Unchanged | Transient bypass | No static bounds cache |
| --- | ---: | ---: | ---: |
| Retained bounds entries / points | 2,861 / 99,975 | 1,820 / 100,000 | 0 / 0 |
| Post-GC Java used heap | 32.4 MiB | 30.6 MiB | 27.8 MiB |
| Post-GC process PSS | 186.8 MiB | 186.1 MiB | 179.6 MiB |
| Input-to-app-draw p50 / p95 | 33.64 / 60.55 ms | 34.02 / 60.80 ms | 35.59 / 61.03 ms |
| Final-window input-to-draw p95 | 31.92 ms | 31.82 ms | 32.00 ms |
| Process CPU | 276.5 s | 277.0 s | 278.5 s |
| Charged process writes | 36.43 MiB | 36.24 MiB | 36.48 MiB |

The transient bypass removes more than 1,000 stale map references and reduces
post-GC Java use by about 1.8 MiB in the paired runs. Its input latency and CPU
remain close to baseline. A separate branch, `perf/no-static-bounds-cache`,
removes the cache entirely and frees another 2.8 MiB of post-GC Java use, but
the higher median input latency and CPU make that a RAM-focused alternative,
not the preferred choice. The unchanged and candidate static caches both hit
the 100,000-point cap, so this result does not establish a lower cap occupancy.

The candidate also passed API 36 emulator live, pen-up, persisted and rebuilt
stroke pixel checks; coverage and mask checks; and exact viewport, edge, pan,
zoom and bounded-cache checks. Single post-GC PSS samples do not establish a
stable system-memory reduction, while cache-entry removal is deterministic.
The benchmark uses synthetic input and excludes physical pen sampling and E Ink
scanout. Raw captures are under `.test-artifacts/hillclimb/` in
`tablet-bounds-cache-probe-baseline-1/`,
`tablet-bounds-cache-probe-candidate-1/`, and
`tablet-no-static-bounds-cache-1/`.
