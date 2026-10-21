# Lazy topmost selection hit test — NoteAir6C, 2026-10-21

The Android editor used to construct a DrawItem for every active item in the selected layer before testing a selection tap, even when the topmost object was the first exact hit. The candidate traverses the same layer/page order from top to bottom, checks active flags, and constructs each DrawItem only when reached. It keeps exact coverage checks and the existing CLI/proxy lease path. Area and multi-item selection still collect all exact hits in the same order.

Two isolated NoteAir6C interaction runs used the same copied note and 35 measured selection taps after five warmups as the [sparse-validation branch](selection-sparse-validation.md). Values are median / p95 milliseconds.

| Metric | Sparse validation, two runs | Lazy hit test, two runs |
| --- | ---: | ---: |
| Input to first updated UI draw | 37.0 / 61.6; 37.5 / 62.3 | 37.5 / 41.7; 38.0 / 42.6 |
| Input to updated Canvas draw | 49.3 / 78.4; 50.1 / 78.5 | 52.8 / 58.6; 53.5 / 60.0 |
| Tap handler | 19.1 / 23.1; 20.7 / 23.5 | 16.5 / 20.4; 17.4 / 20.8 |

The repeated improvement is in the latency tail: UI-draw p95 fell by roughly 20 ms and Canvas-draw p95 by roughly 19 ms. The median UI result and 40-cycle process CPU (5.0–5.1 s parent, 5.1–5.2 s candidate) were roughly unchanged. Charged disk writes remained zero. Sampled PSS changes varied and do not establish a memory benefit.

The Android coverage check compares the old and lazy selection paths over stroke, image, ellipse, masked-stroke, and layer cases, for both topmost and all-hit selection. The full Android drawing suite also exercises tap, drag-box, lasso, leases, and selection previews. Raw traces and APK hashes are under `.test-artifacts/hillclimb/tablet-selection-lazy-{1,2}/`. Timings end at Android drawing, before physical touch sampling and E Ink scanout.
