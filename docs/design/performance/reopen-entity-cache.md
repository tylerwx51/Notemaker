# Reuse stroke JSON objects on same-document reopen — 2026-10-20

The `perf/reopen-entity-cache` branch retains the Android stroke/layer JSON
conversion cache when `load()` opens the same document again. It clears the
cache when the document name changes. `refreshShared()` still compares every
entity with the new replica model and creates a fresh JSON object for changed
content. This avoids rebuilding unchanged stroke wrappers during a warm reopen.

Two NoteAir6C runs used the same disposable captured note and 12 warm reopens
per run, with two warmups. Both physical interaction suites passed, including
durable object history checks. The Android drawing integration suite also
exercised rendering, document switching, and process-restart behavior.

| Warm reopen metric | Direct preferences baseline | Cache retained |
| --- | ---: | ---: |
| First window UI draw median | 33.2–33.9 ms | 22.4–24.2 ms |
| First window UI draw p95 | 37.5–38.1 ms | 25.0–28.2 ms |
| Action handler median | 32.1–32.6 ms | 20.2–21.5 ms |
| Process CPU across 12 reopens | 723–774 ms | 563–622 ms |
| PSS after explicit GC | 144.3 MiB | 144.5–145.1 MiB |

The repeated latency and CPU gains support using the cache for unchanged
entities. The post-GC PSS is 0.2–0.8 MiB higher in these runs, but the sample
is too small to establish a resident-memory cost. As with the earlier reopen
measurements, the note-free counters and APK hashes are in
[`reopen-entity-cache.json`](reopen-entity-cache.json); the prior branch's
measurements are in
[`reopen-direct-preferences.json`](reopen-direct-preferences.json). The suite
does not include the document chooser, physical finger sampling, or E Ink
scanout.
