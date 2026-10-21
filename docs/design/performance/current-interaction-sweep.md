# Current Android interaction sweep — NoteAir6C, 2026-10-21

The `perf/render-prune-on-model-change` branch ran the isolated physical
NoteAir6C interaction suite against the copied `notes` fixture. The suite
measures synthetic input to the first updated Android UI or Canvas draw on
disposable documents. It includes five warmups per 40-action mode, two per
12 reopen actions, and six per 60 history actions. The installed personal app
and note were untouched.

| Action | UI draw p50 / p95 | Canvas draw p50 / p95 | Mode CPU | Charged writes |
| --- | ---: | ---: | ---: | ---: |
| Pan (55 measured) | 11.5 / 29.7 ms | 29.5 / 51.5 ms | 2.13 s | 132 KiB |
| Zoom redraw (35) | 7.5 / 28.1 ms | 24.2 / 44.5 ms | 1.32 s | 0 |
| Tool switch (35) | 24.4 / 25.9 ms | chrome only | 1.06 s | 0 |
| Layer switch (35) | 24.6 / 25.9 ms | chrome only | 2.21 s | 328 KiB |
| Selection tap (35) | 29.8 / 40.0 ms | 46.7 / 55.6 ms | 5.75 s | 0 |
| Document reopen (10) | 22.5 / 27.3 ms | 31.9 / 42.7 ms | 0.61 s | 0 |
| Object undo/redo (54) | 33.3 / 40.8 ms | 44.8 / 55.1 ms | 4.26 s | 1,120 KiB |
| Stroke undo/redo (54) | 34.9 / 38.0 ms | 48.6 / 56.5 ms | 5.17 s | 1,040 KiB |
| Object create (35) | 38.2 / 60.2 ms | 56.3 / 76.5 ms | 3.58 s | 748 KiB |
| Page switch (35) | 20.1 / 21.9 ms | 23.8 / 26.2 ms | 1.69 s | 324 KiB |

Object creation has the largest updated-draw tail in this suite; history
toggles and selection taps follow. The object-create action handler itself is
only 0.4 ms median / 1.0 ms p95, so its latency lies in asynchronous commit,
UI callback, scheduling, and drawing rather than the input handler. The
selected-note PSS samples vary considerably by mode and GC timing: for
example, the object-create mode began at 216.7 MiB and ended at 178.8 MiB.
Those samples do not establish a retained-memory improvement. All modes had
zero charged read bytes; writes include local replica and settings traffic.

The page-switch mode retained two pages of tiles, reused 78 tiles, rendered
two, and performed no cache clear. The suite verified that the page preference
remained durable. Raw private metrics and APK metadata are under
`.test-artifacts/hillclimb/tablet-render-prune-interaction-sweep-1/`.
These timings exclude physical touch sampling and E Ink scanout.
