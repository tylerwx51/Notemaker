# Tablet interaction sweep — 2026-10-20

The isolated NoteAir6C harness loaded the same disposable note for four
alternating runs: old toolbar, candidate, candidate repeat, old toolbar repeat.
Each run performed 60 synthetic pan updates, 40 viewport zoom redraws and 40
tool switches, excluding five warmup actions per mode. The candidate avoids
invalidating toolbar icons whose brush and color did not change. Tool selection
markers and changed labels still update.

| Input to end of app draw, p95 | Old first | Candidate first | Candidate repeat | Old repeat |
| --- | ---: | ---: | ---: | ---: |
| Pan | 30.6 ms | 26.0 ms | 28.8 ms | 26.2 ms |
| Zoom redraw | 26.3 ms | 25.9 ms | 28.9 ms | 26.7 ms |
| Tool switch | 50.5 ms | 32.8 ms | 29.9 ms | 49.8 ms |

Tool switching improved by about 17–20 ms at p95 in both candidate runs. Pan
and zoom values varied without a consistent candidate gain. Tool-switch handler
p95 fell from 9.9/9.4 ms to 6.6/6.1 ms; window-frame p95 fell from 45.4/45.8
ms to 42.0/42.2 ms. Whole-process CPU across the suite was 4.51/4.37 seconds
on the old toolbar and 4.21/4.21 seconds on the candidate. Charged writes were
132 KiB in every run. PSS fell from roughly 221 MiB to 142 MiB within each run,
consistent with one-time activity state being released; these two snapshots do
not establish a steady-state memory improvement.

The zoom case changes the canvas scale directly and excludes pinch recognition.
All timing ends at Android app drawing or window-frame submission, before E Ink
scanout. The emulator tool-switch regression passed, including selection lease
release and preview cancellation. Sanitized counters and APK hashes are in
[`interaction-sweep.json`](interaction-sweep.json); note contents and raw timing
records remain under `.test-artifacts/hillclimb/`.
