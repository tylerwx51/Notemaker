# Single atomic validation for offline selection — NoteAir6C, 2026-10-21

The Android editor selects an exact item ID, then asks its local proxy for a
selection lease. With no upstream proxy configured, `acquireSelection()` used
to read and validate that reference before entering `Store.editTransient()`,
where it read and validated the reference again under the coordination lock.
Branch `perf/offline-select-single-validation` keeps the atomic check and
skips the earlier document read on the offline path. When an upstream exists,
prevalidation remains before any remote lock request. The action and CLI
protocol are unchanged.

The physical NoteAir6C ran the same isolated copied `Drawing object protocol.`
note in A/B/A order: two unchanged-build runs, two candidate runs, then one
more unchanged-build run. Each interaction sweep had 35 measured selection
taps after five warmups and verified state after restart.

| Selection metric | Unchanged A1 / A2 / A3 | Candidate B1 / B2 |
| --- | ---: | ---: |
| Handler-to-Canvas-draw p95 | 52.5 / 53.3 / 49.8 ms | 41.0 / 32.9 ms |
| Input-to-first-UI-draw p95 | 62.9 / 67.3 / 64.2 ms | 62.4 / 59.8 ms |
| Input-to-Canvas-draw p95 | 75.1 / 79.4 / 76.5 ms | 74.8 / 66.3 ms |
| Selection-mode process CPU | 4.83 / 4.79 / 4.82 s | 4.91 / 4.90 s |
| Charged writes | 0 | 0 |

The downstream wait improved in both candidate runs, while the first visible
draw improved modestly and varied between runs. The small CPU increase is not
evidence of a CPU gain. On the older `notes` fixture, first UI-draw p95 was
43.8 ms unchanged and 42.0 ms with the candidate; Canvas-draw p95 stayed
about 60.1 ms. Other interaction modes stayed near their unchanged-build
timings. This supports keeping the candidate as a small selection-latency
improvement without changing online lock behavior.

The 22 CLI checks, 16 proxy checks, and 23 dedicated selection checks passed,
including invalid references, cross-scope sessions, competing locks, and
disconnect propagation. The full API 36 Android drawing suite passed its
selection, lease, preview, clipboard, pixel, gesture, and panel checks. The
source note and raw traces remain private under `.test-artifacts/hillclimb/`:
`tablet-current-interaction-sweep-{1,2}/`,
`tablet-offline-single-validation-{1,2}/`,
`tablet-current-interaction-bracket-1/`, and
`tablet-offline-single-validation-old-fixture-1/`.
Timings end at Android drawing, before physical touch sampling and E Ink
scanout.
