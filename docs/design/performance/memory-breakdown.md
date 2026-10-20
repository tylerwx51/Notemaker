# Physical tablet memory breakdown — 2026-10-20

The isolated NoteAir6C stroke probe on branch `perf/boox-memory-breakdown`
records Android's process memory categories every 100 strokes. The full
sanitized series is in [`tablet-memory-breakdown.json`](tablet-memory-breakdown.json).
All 1,000 strokes survived restart. The run measured 81.37 ms input-to-app-draw
p95, consistent with the 80.59 ms compact-history run; this is measurement
work, not a latency change.

| Checkpoint | Total PSS | Graphics | Java heap | Native heap |
| --- | ---: | ---: | ---: | ---: |
| Start | 103 MiB | 57 MiB | 12 MiB | 12 MiB |
| 100 strokes | 282 MiB | 200 MiB | 31 MiB | 27 MiB |
| 400 strokes, highest sampled PSS | 404 MiB | 228 MiB | 92 MiB | 61 MiB |
| 1,000 strokes | 290 MiB | 148 MiB | 87 MiB | 31 MiB |

Graphics is the largest category at the sampled peak. Java and native heaps
also swing substantially between checkpoints without a forced GC. The tile
cache's live pixel estimate at run end was about 20 MiB, far below its 96 MiB
admission cap. Android's graphics category includes rendering allocations beyond
those counted by the tile cache, so the category does not identify one owner.
Reducing only the cache cap cannot be assumed to save the 228 MiB graphics
allocation observed here.

The samples are process PSS snapshots, not high-water marks between checkpoints.
`Debug.MemoryInfo` categories use Android's accounting and can include shared
graphics resources; they do not prove a leak. The workload uses synthetic
stylus dispatch and does not measure E Ink scanout or a user's pen input.
