# Current 1,000-stroke baseline — NoteAir6C, 2026-10-20

The physical NoteAir6C ran the isolated synthetic 1,000-stroke workload on
`perf/tool-switch-chrome-only`. Every stroke survived restart, and the
incremental model matched full replay. This establishes the sustained-ink
baseline before the next resource experiments; tool switching itself is not
part of this workload.

| Measure | Result |
| --- | ---: |
| Overall input to app draw p95 | 60.83 ms |
| Input to app draw p95, strokes 1–100 / 401–500 / 901–1,000 | 30.58 / 31.25 / 31.84 ms |
| Final-window full frame p95 | 91.09 ms |
| Final-window app draw / command issue p95 | 11.9 / 49.5 ms |
| Process CPU over workload | 277.9 s |
| Charged writes | 36.4 MiB |
| Peak sampled process PSS | 264.2 MiB |
| Tile repaints / item paints | 7,826 / 241,943 |

The full-frame tail is still dominated by Android command issue rather than
app draw. Resource samples were taken every 100 strokes, so the PSS figure is
the highest sample, not a continuous high-water mark. Input is synthetic;
stylus hardware sampling and E Ink scanout are outside these timings. Raw
metrics, resource samples, replay check, and APK hash are under
`.test-artifacts/hillclimb/tablet-tool-switch-strokes-1/`.
