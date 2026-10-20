# Model copies during raw append — NoteAir6C, 2026-10-20

`Store.append()` copied the current model to check deletion, again inside `Collaboration.requireUnlocked()`, and again to record version bases. The preferred `perf/append-model-reuse` branch reuses one model for deletion and version-base checks, reducing three copies to two without changing the lease check. The separate `perf/append-model-once` branch also passes that model into the lease check, reducing three copies to one. Neither changes the action, journal, or live-stroke update cadence.

| Physical 1,000-stroke run | Earlier baseline | Two copies 1 | Two copies 2 | One copy, passing run |
| --- | ---: | ---: | ---: | ---: |
| Input-to-app-draw p95 | 69.7 ms | 68.0 ms | 67.5 ms | 68.4 ms |
| Elapsed | 304.4 s | 284.3 s | 286.1 s | 287.0 s |
| Process CPU delta | 311.2 s | 290.6 s | 291.8 s | 293.9 s |
| Charged writes | 37.9 MiB | 36.8 MiB | 36.6 MiB | 36.9 MiB |
| Peak sampled PSS | 267 MiB | 265 MiB | 279 MiB | 244 MiB |

The one-copy build had another full trial that **failed**: after 1,000 gestures and writer drain, its effective and replayed models contained only 999 strokes. No save error appeared in the captured app log. Its passing trial shows no measurable advantage over the two-copy build. The extra lease-check shortcut is rejected while that durability failure is unresolved.

The two-copy version passed both full runs, including replay equivalence and 1,000 strokes after process restart. CLI and proxy suites passed for both versions (22 and 15 checks). The historical baseline uses the same device and input fixture but an earlier revision with the 100 ms live-update cadence; its different parser and checkpoint code limits attribution of small time differences. The evidence establishes no apparent latency regression and modestly lower CPU/elapsed counters; it does not isolate their exact cause. APK hashes and note-free counters are in [append-model-copies.json](append-model-copies.json). The benchmark synthesizes app input and does not measure E Ink scanout.
