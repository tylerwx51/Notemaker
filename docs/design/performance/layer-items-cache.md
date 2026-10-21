# Cached layer item order — NoteAir6C, 2026-10-21

The model already indexes items by layer and page, but every selection tap
copied and sorted the entire scoped list. This branch reuses the immutable
ordered list until an item is added, replaced, or moved in either affected
scope. Deleting the document clears the cache. Active flags are checked by
the callers, so toggling visibility does not change the list contents.

An isolated NoteAir6C interaction sweep used the same copied `Drawing object
protocol.` fixture on the parent and candidate. Each run had 35 measured
selection taps after five warmups; the tap selected the same topmost rectangle
and acquired a local lease. Values are median / p95 milliseconds.

| Selection tap | Parent | Cache run 1 | Cache run 2 |
| --- | ---: | ---: | ---: |
| Tap action | 20.0 / 22.7 | 3.0 / 3.2 | 2.9 / 3.1 |
| Input to first updated UI draw | 42.6 / 44.7 | 20.1 / 40.0 | 19.2 / 34.6 |
| Input to updated Canvas draw | 51.4 / 55.4 | 30.3 / 52.6 | 30.1 / 47.0 |

The repeated action reduction is large; frame scheduling makes the draw
latency more variable. Selection-mode process CPU fell from 5.88 s on the
parent to 5.58 s on each candidate run. Charged writes stayed zero. Sampled
PSS after the mode was 182.3 MiB on the parent and 180.4 MiB on each candidate;
these single samples include GC timing and do not prove retained-memory
savings. Other interaction modes stayed near their previous ranges.

The candidate passed 22 CLI, 17 proxy, 11 coverage, and 23 selection checks,
plus the full Android drawing suite. A direct JVM check also exercised cache
reuse, item replacement, page and layer moves, stroke-before-object ordering,
and document deletion. Its source is kept at
`.test-artifacts/hillclimb/LayerItemsCacheCheck.java`.
Raw private traces and APK hashes
are under `.test-artifacts/hillclimb/tablet-layer-items-{parent-1,cache-{1,2}}/`.
These are synthetic Android interaction timings, before physical touch input
and E Ink scanout. The installed personal app and note were untouched.
