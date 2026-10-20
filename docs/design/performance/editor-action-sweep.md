# Physical editor action sweep — 2026-10-20

The isolated NoteAir6C harness ran three copies of the same captured note.
Beyond the earlier pan, zoom redraw, and tool switch cases, it added 40 switches
between the current layer and an empty layer created through the local CLI, and
12 warm reopens through the Activity load path. Five actions per mode were
warmups, except two reopens. Each action waited for a real Canvas draw; Android
window frame metrics and process counters were sampled separately. The source
note in the installed app was untouched.

| Action | Input-to-app-draw median | Input-to-app-draw p95 | Handler p95 | Window frame p95 |
| --- | ---: | ---: | ---: | ---: |
| Pan | 24.3–24.4 ms | 27.5–36.3 ms | 3.5–3.6 ms | 26.8–28.6 ms |
| Zoom redraw | 23.2–23.9 ms | 29.0–30.7 ms | 1.7–3.1 ms | 25.3–25.8 ms |
| Tool switch | 24.2–25.2 ms | 30.8–47.4 ms | 5.7–9.2 ms | 32.8–38.0 ms |
| Layer switch | 24.3–25.3 ms | 46.2–50.8 ms | 22.2–28.4 ms | 90.4–91.1 ms |
| Reopen same note | 53.2–54.3 ms | 55.2–62.2 ms | 52.6–59.4 ms | 57.7–88.8 ms |

Layer switching is repeatably slower than pan or zoom at p95. The empty-layer
fixture means these timings mostly cover preference persistence, toolbar refresh,
and a redraw of the original note, rather than painting content on the new
layer. The 40 layer switches charged roughly 328–332 KiB of writes per run.
Twelve note reopens caused about 2.66 MiB of logical process reads, with no
charged disk reads in the page-cache-warm runs. Reopens also raised sampled PSS
by about 33 MiB, but in the third run an explicit GC reduced PSS from 190 MiB
to 138 MiB and Java used heap from 67 MiB to 14 MiB. This supports allocation
churn, not retained-memory growth from those 12 reopens. The earlier pan mode's
large PSS drop also reflects collection of first-load state; these per-mode
snapshots should not be treated as steady-state peaks.

Timing starts when a synthetic action is queued and ends at app drawing; it
excludes physical finger sampling and E Ink scanout. Zoom changes scale directly
and excludes pinch recognition. Reopen calls the Activity load path and excludes
document-chooser interaction. Tool-switch tails varied between runs. The
sanitized timings, counters, and APK hashes are in
[`editor-action-sweep.json`](editor-action-sweep.json); the note and raw traces
remain private under `.test-artifacts/hillclimb/`.
