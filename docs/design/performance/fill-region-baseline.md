# Enclosed-area fill baseline — NoteAir6C, 2026-10-21

The isolated physical interaction suite now includes 20 consecutive taps
inside a 200 × 160 px rectangle on a disposable, otherwise blank document.
Four taps warm the mode; 16 are measured. Each tap uses Android's normal
`fillAt` path through the shared local replica, waits for a durable new SVG
region and updated UI and Canvas draws, then checks all 20 regions and the
editor undo history. Existing ten interaction modes still pass. The growing
set of filled regions is a sustained-overdraw case, not an isolated single
fill, and the benchmark does not measure touch sampling or E Ink scanout.

| Measure | Baseline |
| --- | ---: |
| Input to updated UI draw p50 / p95 | 45.7 / 65.5 ms |
| Input to updated Canvas draw p50 / p95 | 80.2 / 110.9 ms |
| Tap action p50 / p95 | 1.6 / 2.7 ms |
| Window draw phase p95 | 42.4 ms |
| Window command-issue phase p95 | 47.9 ms |
| Mode process CPU | 2.23 s |
| Charged writes | 488 KiB |
| Sampled PSS before / after | 167.7 / 205.4 MiB |

This baseline establishes both the asynchronous fill and rendering costs;
the fill loop's per-pixel temporary arrays are one candidate for reducing
worker CPU. PSS samples include collection timing and do not prove retained
memory growth. Raw counters, APK hash, and the disposable replica are under
`.test-artifacts/hillclimb/tablet-fill-region-baseline-1/`.
