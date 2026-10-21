# One-frame local settings acknowledgment — NoteAir6C, 2026-10-20

Local settings saves still run through the shared replica and durable journal.
The editor already updates its page, layer, or undo state before queuing that
write. On a successful local acknowledgment whose snapshot contains no new
document, selection, or connection state, the candidate accepts the new
revision without issuing a second UI frame. Errors and changed snapshots
still refresh the UI. Remote settings retain the existing saving indicator
and redraw path. Fast local settings saves no longer flash a saving label.

Two isolated physical NoteAir6C runs used the same copied note and 35 measured
actions per page/layer mode after warmup. The comparison uses two runs of the
same expanded interaction suite before the change.

| Input to first updated window draw, p95 | Baseline runs | Candidate runs |
| --- | ---: | ---: |
| Layer switch | 49.1 / 49.9 ms | 26.1 / 25.5 ms |
| Populated-page switch | 42.9 / 40.8 ms | 18.1 / 19.7 ms |
| Empty-page switch | 45.1 / 45.6 ms | 23.7 / 23.7 ms |
| Object undo/redo | 86.6 / 91.6 ms | 72.4 / 71.4 ms |
| Unchanged settings save | 45.3 / 45.9 ms | 23.9 / 24.4 ms |

Layer-switch sampled window frames fell from 60–64 to 42–43 per mode, and
process CPU from 2.48 seconds to 2.18–2.29 seconds. Settings writes were
unchanged at roughly 320–332 KiB per 40-action mode; this is a redraw/CPU
improvement, not an I/O saving. The 1,000-stroke physical gate passed replay
equivalence and restart durability with 60.9 ms input-to-app-draw p95 and
36.4 MiB charged writes, matching earlier 60.9–61.0 ms and 36.4–36.7 MiB
runs. Peak sampled PSS was 268 MiB versus 243–259 MiB in those earlier runs;
that single-candidate sample cannot establish a memory change.

The full Android emulator drawing suite passed pixel, selection, clipboard
restart, layer, and toolbar checks. The isolated interaction suite verified
durable page and history state. Measurements end at Android app/window draw,
before physical E Ink scanout. The copied note and raw traces remain private
under `.test-artifacts/hillclimb/`.

The clean `perf/settings-single-frame` branch contains only the production
change and this report. Its rebuilt physical APK repeated the result:
layer/page/history window-draw p95 was 25.8/19.0/70.5 ms, with durable page
and history checks passing. The benchmark-only controls remain on the
experiment branches.
