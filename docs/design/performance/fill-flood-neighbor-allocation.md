# Allocation-free flood neighbors — NoteAir6C, 2026-10-21

`RegionFill.create` used to allocate a four-element `int[]` for every pixel
removed from its flood queue. The fill now checks the same four neighbors in
the same order with scalar indices. Region boundaries, output SVG, CLI action,
and local replica semantics are unchanged. The new `fill_region` interaction
mode on `perf/fill-region-baseline` provided a physical control.

Two preferred-parent/candidate pairs ran on the NoteAir6C with the same
isolated app, blank rectangle fixture, and 20 consecutive fills per run.
Each run discarded four warmups, waited for durable updated UI and Canvas
draws, and verified all regions and editor history. Values are in run order.

| Measure | Parent 1 / 2 | Candidate 1 / 2 |
| --- | ---: | ---: |
| Input to UI draw p95 | 65.5 / 86.9 ms | 66.7 / 59.2 ms |
| Input to Canvas draw p95 | 110.9 / 139.0 ms | 134.9 / 88.8 ms |
| Mode process CPU | 2.23 / 2.37 s | 2.17 / 2.03 s |
| Java heap used after mode | 73.1 / 73.6 MiB | 55.5 / 54.5 MiB |
| Process PSS after mode | 205.4 / 206.4 MiB | 188.1 / 186.9 MiB |
| Charged writes | 488 / 488 KiB | 488 / 488 KiB |

The two candidate runs used less CPU and about 18 MiB less sampled Java heap.
Heap and PSS are single post-mode samples subject to collection timing, so
they do not establish retained-memory savings. The lower allocation pressure
matches the code change. UI and Canvas p95 move in opposite directions in the
two pairs; no visible-latency improvement is established. Keep this candidate
as the preferred branch for its repeatable resource benefit and unchanged
durable output. The shared CLI drawing and coverage suites passed (14 and 11
checks), as did the full Android drawing suite including tap-to-fill and pixel
coverage. Raw metrics and APK hashes are under
`.test-artifacts/hillclimb/tablet-fill-{region-baseline,flood-neighbors}-{1,2}/`.
Timings exclude physical touch sampling and E Ink scanout.
