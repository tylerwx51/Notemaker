# Current tablet interaction sweep — 2026-10-21

The `perf/current-interaction-sweep` branch measures ten editor actions on the
physical NoteAir6C with the isolated app package. Two runs use the copied
`Drawing object protocol.` note; a third uses the older `notes` fixture from
the selection-latency study. Each mode excludes its warmup actions and waits
for the expected updated view. Original app data remains untouched.

| Action | Current note UI-draw p95, runs 1 / 2 | Older note UI-draw p95 |
| --- | ---: | ---: |
| Pan | 25.3 / 28.2 ms | 29.8 ms |
| Zoom redraw | 18.3 / 14.6 ms | 29.7 ms |
| Tool switch | 25.7 / 25.2 ms | 25.3 ms |
| Layer switch | 26.1 / 25.7 ms | 25.9 ms |
| Selection tap | 62.9 / 67.3 ms | 43.8 ms |
| Warm document reopen | 28.3 / 29.5 ms | 29.3 ms |
| Object history toggle | 43.8 / 45.4 ms | 41.6 ms |
| Stroke history toggle | 43.6 / 42.8 ms | 40.7 ms |
| Rectangle creation | 60.0 / 60.6 ms | 58.7 ms |
| Page switch | 21.6 / 21.9 ms | 22.4 ms |

Selection and object creation remain the slowest first updated UI draws.
Selection is strongly fixture-dependent: the older note's 43.8 ms p95 matches
the earlier 41.7–42.6 ms lazy-hit-test result on that fixture. The current
note's 63–67 ms result is not evidence of a regression without a same-fixture
older-build comparison. Its selection handler p95 was 40.2/34.1 ms, compared
with 20.3 ms on the older note. Canvas-draw p95 for selection was 75.1/79.4 ms
on the current note and 60.1 ms on the older note. Rectangle creation reached
Canvas in 74.5/76.0 ms p95 on the current note.

The first current-note run charged about 328 KiB for 40 layer switches,
1,120 KiB for 60 object-history toggles, 1,032 KiB for 60 stroke-history
toggles, and 756 KiB for 40 object creations. Pan and tool switch charged no
disk writes. PSS before/after each mode is affected by GC and should not be
interpreted as action-specific retained memory. Tool and layer switching are
chrome-only in this harness and have no updated Canvas-draw samples.

These are synthetic actions and Android UI/Canvas/frame timings, excluding
physical touch sampling and E Ink scanout. The source note is not included in
the repository. Raw traces and APK hashes are under `.test-artifacts/hillclimb/`
in `tablet-current-interaction-sweep-{1,2}/` and
`tablet-current-selection-original-fixture-1/`.
