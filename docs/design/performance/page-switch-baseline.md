# Physical page-switch baseline — 2026-10-20

The isolated NoteAir6C interaction suite now creates a disposable pages
document with one rectangle on each of two pages. It alternates page 0 and page
1 forty times, omitting five warmups. The action runs the same deselection,
gesture reset, page preference save, and UI update as the page dialog after the
user chooses a page; numeric entry and dialog interaction are excluded. The
suite verifies that the final page preference is durable through the local
replica. The installed app and captured source note are untouched.

Three physical runs measured first window UI-draw p50 of 17.0–17.3 ms and p95
of 40.4–41.4 ms. Canvas draw p95 was 49.3–51.9 ms. The handler itself had a
1.9 ms median and 3.5 ms p95 in the first run; its queue wait reached 20.6–24.4
ms p95 across the first two runs. Frame command issue reached 46.7–47.2 ms p95,
with full-frame p95 of 85.5–85.9 ms. Frame samples include background work and
are not paired one-for-one with page actions.

The first two runs charged 324 KiB of writes for 40 switches, had no charged
disk reads, and used 2.22–2.24 s of process CPU during the mode. Sampled PSS
rose by about 41–42 MiB, then fell below its pre-mode value after explicit GC;
that pattern does not establish retained growth. A third run added tile-cache
counters: 80 tile repaints and 80 tile allocations for the 40 switches, with
only about 2.0 MiB of tiles resident at the end. The renderer currently clears
the ink tile cache on every page change, so returning to a recent page repaints
it. A bounded cross-page cache is the next experiment.

Note-free metrics, resources, and APK hashes are in
[`page-switch-baseline.json`](page-switch-baseline.json). Input is synthetic;
physical touch sampling and E Ink scanout were not measured.
