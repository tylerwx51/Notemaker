# Physical selection-tap baseline — NoteAir6C, 2026-10-21

The isolated editor interaction suite now places a filled rectangle at the end of the copied note's original layer, then taps it 40 times (5 warmups). Each measured tap must acquire a local selection lease, select the fixture object, and reach an updated UI and Canvas draw. Between samples the harness deselects, drains the writer, and verifies that the lease was released. The installed personal app and note remain untouched.

| Metric | Run 1 median / p95 | Run 2 median / p95 |
| --- | ---: | ---: |
| Input to first updated UI draw | 84.2 / 90.5 ms | 80.1 / 89.8 ms |
| Input to updated Canvas draw | 93.9 / 108.5 ms | 91.6 / 103.4 ms |
| Tap handler | 16.8 / 21.1 ms | 16.6 / 21.1 ms |
| Handler to Canvas draw | 76.5 / 93.8 ms | 72.1 / 87.9 ms |

Forty acquire/release cycles used 6.20–6.21 seconds of process CPU and charged no disk writes in either run. This points to in-process selection work, with substantial time after the tap handler. In the current local proxy path, selection validates the requested scope before acquisition, repeats scope validation when it enters the shared selection method, and validates all layer items again inside the atomic coordination update. The next experiment will remove redundant full-layer item construction while preserving validation inside that atomic update.

Raw traces, resource counters, and APK hashes are under `.test-artifacts/hillclimb/tablet-selection-tap-baseline-{1,2}/`. The tap is synthetic and timings end at Android drawing, before E Ink scanout.
