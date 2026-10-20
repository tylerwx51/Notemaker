# Physical tablet stroke baseline — 2026-10-20

The isolated 1,000-stroke benchmark passed replay and restart checks on the
NoteAir6C. All 1,000 strokes were durable. This run gives the first process RAM
and I/O series for the tablet workload; the raw capture stays in
`.test-artifacts/hillclimb/tablet-baseline-4/`. The sanitized counters are in
[`tablet-strokes-baseline.json`](tablet-strokes-baseline.json), with the built
APK's SHA-256 for comparison against later branches.

| Metric | Measured result |
| --- | ---: |
| Enqueue to completed app draw, p50 / p95 / max | 50.2 / 82.2 / 128.8 ms |
| Input handler p95, first / middle / final 100 strokes | 7.5 / 7.8 / 2.6 ms |
| Completed app draw p95, first / middle / final window | 41.1 / 47.2 / 54.4 ms |
| Window frame p95, first / middle / final window | 90.3 / 93.8 / 92.5 ms |
| Sampled process PSS, start / peak / 1,000 strokes | 103 / 438 / 345 MiB |
| Charged process writes through 1,000 strokes | 62.0 MiB |
| Save queue maximum / final drain | 3 / 993 ms |

The first failed attempt placed later strokes below the tablet's canvas and
could not confirm a final draw. The workload now wraps its grid within the
actual canvas viewport. An earlier completed run then exceeded Android's
instrumentation transaction limit by returning the full metrics as text;
instrumentation now returns a short status and the runner reads its files.
Neither failed run is used as a latency baseline.

These timings end at app rendering, not physical E Ink scanout. The synthetic
events are dispatched by instrumentation, so they do not measure stylus sampling.
RAM samples include the process and graphics allocations reported by Android;
they were taken between stroke groups without forced GC. `/proc/self/io`
`write_bytes` is charged I/O, not measured flash latency, and the 1,000-stroke
sample precedes final writer drain. The 2,279 dispatch waits over 30 ms suggest
UI scheduling and rendering are more urgent than input handler code alone.
Physical runs need repeats before small branch differences can be trusted.

The focused API 36 emulator check also passed all 7,920 measured inputs under
30 ms (p95 17.41 ms, maximum 28.68 ms), including replay and restart. Its peak
sampled PSS was 116 MiB. This contrast reinforces the need to use the tablet
for RAM and frame decisions; the emulator does not reproduce the BOOX display
stack or its scheduling.
