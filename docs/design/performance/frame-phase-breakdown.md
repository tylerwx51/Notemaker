# BOOX window frame phases — 2026-10-20

The isolated physical NoteAir6C stroke harness now records Android
`FrameMetrics` phases on a dedicated listener thread, alongside its existing
input-to-app-draw and resource counters. The same 1,000 synthetic strokes ran
on `perf/frame-phase-breakdown`; all strokes survived replay and process
restart. This run's overall input-to-app-draw p95 was 68.7 ms, close to the
preceding branch's 68.6 ms.

| Phase, p95 | Strokes 1–100 | Strokes 401–500 | Strokes 901–1,000 |
| --- | ---: | ---: | ---: |
| Window frame total | 87.1 ms | 89.4 ms | 91.1 ms |
| App draw | 12.0 ms | 18.3 ms | 30.7 ms |
| Command issue | 48.1 ms | 49.9 ms | 49.9 ms |
| Renderer sync | 3.5 ms | 3.2 ms | 2.5 ms |
| Buffer swap | 2.3 ms | 2.0 ms | 1.5 ms |

Command issue is the largest reported phase even early in the run. App draw
grows with document density, reaching 30.7 ms p95 late in the run. Percentiles
of separate phases cannot be added to reconstruct a frame percentile. This
indicates that optimizing Java traversal alone has a roughly 50 ms frame-phase
floor on this device under this workload; it does not prove which GPU or BOOX
driver operation consumes the command-issue time. The tile cache held 20.3 MiB
of live pixels at the end, repainted 7,855 tiles and reused resident tile
bitmaps for 7,835 of those repaints. Peak sampled process PSS was 244 MiB and
charged writes were 37.2 MiB. Device and exact APK hashes are in
[`tablet-frame-phases.json`](tablet-frame-phases.json). These figures do not
measure physical stylus sampling or E Ink scanout.
