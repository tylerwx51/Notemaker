# Skip unchanged Canvas draws for lease notifications — NoteAir6C, 2026-10-21

The local-state listener redrew the Canvas for every coordination change,
including lease-only changes. Selection acquire and release already redraw
when local selection state changes; a lease notification does not change the
pixels. `refreshFrom` now updates lease state and editor chrome without
invalidating the Canvas. Transform preview changes still invalidate it.
Lease arbitration, proxy commands, and durable actions are unchanged.

The physical-device interaction suite selected and released the same visible
object 40 times in its isolated selection mode, with five warmups and 35
measured taps. Six recent preferred-branch runs used the same fixture; two
candidate runs measured this change.

| Selection-mode measure | Preferred, 6 runs | Candidate 1 / 2 |
| --- | ---: | ---: |
| Window frames, 40 selections/releases | 160 in every run | 81 / 80 |
| Window frame p95 | 69.5–94.7 ms | 41.8 / 40.5 ms |
| Command-issue frame phase p95 | 42.3–51.5 ms | 6.7 / 6.6 ms |
| Mode process CPU | 5.62–5.88 s | 4.00 / 3.70 s |
| Input to updated UI draw p95 | 39.5–41.0 ms | 40.3 / 41.3 ms |
| Input to updated Canvas draw p95 | 55.1–58.3 ms | 59.4 / 57.2 ms |
| Charged writes | 0 KiB | 0 KiB |

The frame and CPU reductions repeated, while first-draw latency did not
improve. The candidate's Java heap and PSS samples were about 3–4 MiB lower
after selection mode, but single post-mode samples cannot establish retained
memory savings. The full Android drawing suite passed, including foreign
preview/cancellation, lease handling, selection transforms, and fill.

Raw metrics and APK hashes are under
`.test-artifacts/hillclimb/tablet-{fill-merge-vertical-runs-{1,2},fill-squared-parent-1,fill-no-pending-frame-{1,2},fill-delayed-pending-1,selection-lease-refresh-{1,2}}/`.
Android timings exclude physical touch sampling and E Ink scanout.

## Larger-note cross-check

The copied `Drawing object protocol.` note has different selection and tile
costs. A fresh preferred-parent run and two candidate runs used that same
fixture:

| Selection-mode measure | Parent | Candidate 1 / 2 |
| --- | ---: | ---: |
| Window frames, 40 selections/releases | 160 | 80 / 80 |
| Mode process CPU | 5.49 s | 3.72 / 3.84 s |
| Input to updated UI draw p95 | 35.3 ms | 40.1 / 36.7 ms |
| Input to updated Canvas draw p95 | 48.2 ms | 52.9 / 49.4 ms |
| Window frame p95 | 91.4 ms | 91.5 / 91.3 ms |

The frame and CPU gains carry over. The larger note did not improve first-draw
or per-frame tail latency; the candidate's first-draw p95 was 1–5 ms higher
than this single paired parent run. Its tile cost dominates the remaining
window-frame tail. These additional APKs and metrics are under
`.test-artifacts/hillclimb/tablet-selection-lease-current-{parent-1,note-{1,2}}/`.
