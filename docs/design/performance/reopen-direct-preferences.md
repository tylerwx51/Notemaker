# Direct preference reads on document reopen — 2026-10-20

The `perf/reopen-direct-preferences` branch reads the saved hidden layers,
undo/redo lists, selected layer, and page directly from the replica snapshot.
Previously `load()` wrapped the entire snapshot in an Android `JSONObject` to
read those five fields, recursively converting the drawing model as well. The
change leaves the local replica and subsequent `refreshShared()` path intact.

The NoteAir6C interaction suite reopened the same disposable captured note
12 times per run, with two warmups. Both candidate runs passed the full action
suite and the Android emulator's full drawing integration suite, including
process-restart clipboard and layer preference checks.

| Warm reopen metric | Preferred baseline, two runs | Direct preferences, two runs |
| --- | ---: | ---: |
| First window UI draw median | 51.4–52.5 ms | 33.2–33.9 ms |
| First window UI draw p95 | 52.9–54.0 ms | 37.5–38.1 ms |
| Action handler median | 49.5–51.0 ms | 32.1–32.6 ms |
| Process CPU across 12 reopens | 887–902 ms | 723–774 ms |
| Logical bytes read | 2.66 MiB | 2.66 MiB |
| Charged disk reads/writes | 0 / 0 | 0 / 0 |

The near-identical logical I/O and 16–19 ms handler improvement support the
removed whole-snapshot conversion as the cause of the latency reduction. Both
variants finished near 144 MiB PSS after explicit GC. The before/after heap
samples during the reopen mode are affected by collection timing, so they do
not establish an allocation or resident-memory saving. Frame p95 also varied
between runs; the reproducible gain is in action-to-draw latency.

Note-free candidate metrics and APK hashes are in
[`reopen-direct-preferences.json`](reopen-direct-preferences.json). Baseline
metrics are in [`history-action-sweep.json`](history-action-sweep.json). The
input is synthetic and does not include document-chooser interaction, physical
touch sampling, or E Ink scanout.
