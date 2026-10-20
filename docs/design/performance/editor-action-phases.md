# Physical editor action phase timing — 2026-10-20

The `perf/editor-action-phases` branch splits each isolated NoteAir6C action's
enqueue-to-draw latency into UI queue wait, time inside the action handler, and
the subsequent wait to Canvas draw. It also samples Android frame draw and
command-issue phases. The captured-note workload and app renderer are unchanged
from `perf/editor-action-sweep`.

| Mode, p95 | UI queue | Action handler | Handler to app draw | Frame draw | Command issue | Full frame |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Pan | 1.2 ms | 2.5 ms | 26.3 ms | 13.6 ms | 3.3 ms | 25.8 ms |
| Zoom redraw | 0.9 ms | 0.9 ms | 26.2 ms | 12.6 ms | 3.3 ms | 25.2 ms |
| Tool switch | 1.0 ms | 6.7 ms | 25.1 ms | 16.8 ms | 11.3 ms | 40.9 ms |
| Layer switch | 25.0 ms | 3.2 ms | 29.2 ms | 8.9 ms | 48.5 ms | 90.4 ms |
| Same-note reopen | 1.4 ms | 52.4 ms | 3.3 ms | 11.0 ms | 43.4 ms | 62.9 ms |

Layer switching's tail is dominated by waiting to enter the main thread and
by frame command issue, rather than the switch method itself. A local settings
write and associated redraws occur around each switch; the frame listener
includes those frames, so its phase percentiles are not paired one-for-one
with each input latency sample. Reopening the note instead spends most of its
time inside `load()`. This is one physical run, and p95 values from separate
phases are not additive. Sanitized counters and APK hash are in
[`editor-action-phases.json`](editor-action-phases.json). No physical finger
sampling or E Ink scanout was measured.

The harness now also records the first window UI draw. This avoids waiting for
an incidental later Canvas draw when an action changes only editor chrome. On
the preferred renderer, a separate physical run measured layer-switch UI-draw
p50/p95 of 24.9/49.4 ms, close to the earlier Canvas-draw result. Its note-free
metrics and APK hash are in
[`editor-ui-draw-baseline.json`](editor-ui-draw-baseline.json).
