# Commit final stroke and editor history together — NoteAir6C, 2026-10-21

Android previously queued a final `append -` at pen-up and then a separate
durable `settings -` edit for its editor undo list. For a local selected proxy,
this branch uses `append settings -` to make the final stroke update and local
editor settings one journal transaction. It leaves incremental stroke appends
and remote selected proxies on their existing paths. The new structured CLI
command uses the same Store action and settings validation as the separate
commands; rejected input changes neither half of the transaction.

Two isolated 1,000-stroke runs used the physical NoteAir6C. Each dispatched
7,920 measured stylus inputs, checked incremental/full replay equality,
restarted the app, and confirmed 1,000 strokes. The second run also verified
1,000 editor and stored undo entries after restart. A fresh run of the
immediate parent used the same workload; a recent parent run provides a second
reference.

| Measure | Parent, recent / fresh | Candidate 1 / 2 |
| --- | ---: | ---: |
| Input to app draw p50 | 40.7 / 40.7 ms | 32.0 / 31.8 ms |
| Input to app draw p95 | 62.0 / 62.3 ms | 62.8 / 63.1 ms |
| Last 100 strokes, input to draw p95 | 32.4 / 31.8 ms | 39.0 / 38.6 ms |
| Last 100 strokes, app draw phase p95 | 12.5 / 12.2 ms | 21.5 / 21.0 ms |
| Last 100 strokes, full window frame p95 | 91.7 / 91.9 ms | 91.5 / 91.4 ms |
| Process CPU, strokes 0–1,000 | 293.4 / 293.1 s | 280.7 / 279.0 s |
| Charged writes before final drain | 37.16 / 37.48 MiB | 28.93 / 28.51 MiB |
| Post-GC PSS | 185.6 / 185.2 MiB | 183.6 / 183.3 MiB |

The repeated I/O reduction is 8.2–9.0 MiB (22–24%), with about 4–5% lower
process CPU. Median input-to-draw improved, while overall p95 is about 1 ms
higher and the late-window app draw and input-to-draw p95 regress by about
7–9 ms. Full-window p95 did not change materially. The late draw tradeoff is
real in this workload and should be considered alongside the resource gain;
the similar post-GC PSS samples do not establish memory savings.
Retain this branch as the current performance build because the write and CPU
gains repeat without a material full-frame p95 change. Keep
`perf/selection-lease-refresh` as the lower late-draw alternative.

The full Android drawing suite passed, including persisted/rebuilt pixels,
selection, eraser, fill, templates, and clipboard restart. The real CLI
drawing (15 checks) and broader CLI (22 checks) suites passed. A direct CLI
test verified atomic failure, the returned operation, and editor undo after
proxy restart. The sanitized second-run measurements are in
[`stroke-final-append-settings.json`](stroke-final-append-settings.json).
The candidate was captured with working-tree edits on parent `5009572` and
committed as `ecf6dd2`; the summary's source revision is therefore the parent,
while its APK hash identifies the measured build.
Private raw metrics and APK hashes are under
`.test-artifacts/hillclimb/tablet-{selection-lease-strokes-1000-2,stroke-append-settings-1000-{1,2},stroke-append-settings-parent-1000-1}/`.
These synthetic timings exclude physical stylus sampling and E Ink scanout;
charged writes are not measured flash latency.
