# Physical eraser check on the current build — 2026-10-20

The committed baseline build (`perf/device-resource-baseline`) and current
`perf/live-stroke-map` build ran the same isolated BOOX eraser harness against
the same captured note archive. Each operation was undone, and every run
verified that source geometry was restored. The archive produced eight valid
targets per mode; these are small sequential samples. In this sample size,
nearest-rank p95 is the slowest operation.

| Mode and metric | Baseline p50 / p95 | Current p50 / p95 |
| --- | ---: | ---: |
| Partial erase, input to preview draw | 24.2 / 42.4 ms | 21.3 / 35.9 ms |
| Partial erase, input to updated draw | 97.7 / 153.3 ms | 96.2 / 129.7 ms |
| Whole-item erase, input to preview draw | 49.1 / 74.7 ms | 41.6 / 70.5 ms |
| Whole-item erase, input to updated draw | 106.7 / 147.5 ms | 92.6 / 127.4 ms |

The current build shows no obvious erase regression on this fixture and was
faster in these runs. Eight samples cannot establish a stable percentile or
attribute the difference to a particular change. This archive is a later
capture than the 29-target experiment in `docs/design/latency/report.md`, so
those published timings are not a direct before/after comparison. The present
result also remains above the under-30-ms feedback goal, especially for
whole-item erasing. Physical E Ink scanout was not measured.

The note-free counters and exact APK hashes are in
[`erase-current-comparison.json`](erase-current-comparison.json). Raw note and
timing artifacts stay under `.test-artifacts/hillclimb/`.
