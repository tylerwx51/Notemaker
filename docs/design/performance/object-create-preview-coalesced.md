# Coalesced object-creation preview — NoteAir6C, 2026-10-21

Object creation previously waited for the selected proxy's durable local
commit before the new shape appeared. An earlier direct-Canvas preview cut
first-visible latency but drew a second full frame after commit and changed
edge pixels. This branch puts a pending new object through the same document
tile renderer as committed objects. It remains outside the replica and actor
history. When the local commit changes no other painted content and the
canonical object matches, the already drawn tile pixels are retained and only
editor chrome is refreshed. A failed, remote, or mismatched result removes the
preview and redraws from authoritative state.

The physical NoteAir6C interaction suite created 40 rectangles in an isolated
package using the same copied `notes` fixture. Five actions warmed each run;
35 are in the latency distribution. Every object and editor-history entry was
verified in the durable local replica. Candidate Canvas and UI measurements
are **first visible preview** draws; the parent first shows an object after
commit. The candidate separately records the durable UI commit callback.

| Measurement | Preferred parent | Candidate 1 | Candidate 2 |
| --- | ---: | ---: | ---: |
| First visible UI draw p50 / p95 | 38.2 / 60.2 ms | 20.8 / 27.3 ms | 20.4 / 26.6 ms |
| First visible Canvas draw p50 / p95 | 56.3 / 76.5 ms | 35.5 / 53.9 ms | 37.1 / 46.4 ms |
| Durable UI commit callback p95 | — | 62.5 ms | 56.0 ms |
| Window frames during mode | 47 | 45 | 43 |
| Window-frame p95 | 44.4 ms | 47.4 ms | 43.1 ms |
| Process CPU, 40 creations | 3.59 s | 3.13 s | 3.23 s |
| Charged writes, 40 creations | 748 KiB | 744 KiB | 744 KiB |
| Commits reusing preview frame | — | 40 / 40 | 40 / 40 |

The repeated first-visible gain is 22–30 ms at Canvas p95. Frame count stays
near the parent instead of doubling as in the earlier direct preview trial.
Frame p95 and writes do not show a consistent change; sampled PSS moved from
about 216 MiB before object creation to 177–179 MiB afterward in these runs,
reflecting GC timing rather than a retained-memory result. Other interaction
modes were broadly near their prior ranges; their single-run tails do not
establish a causal effect from a preview mode that runs later in the suite.

The emulator coverage check pauses the writer, confirms a changed preview
without a durable object, checks exact preview/committed pixels, and asserts
that a matching commit skips the second Canvas redraw. The full Android
drawing suite passed. Its first attempt hit an emulator startup proxy-owner
race before coverage; the rerun passed all checks. Raw private metrics and
APK metadata are under `.test-artifacts/hillclimb/tablet-object-preview-coalesced-{2,3}/`;
the earlier failed harness-definition attempt is under `...-1/`.
These timings end at Android drawing and do not measure physical touch
sampling or E Ink scanout.

## Sustained drawing and cold-start follow-up

A later 1,000-stroke run on this branch passed durable restart and incremental/
full replay parity. Its overall input-to-app-draw p95 was 61.78 ms and process
CPU was 285.5 s, compared with 61.13–61.21 ms and 282.5–282.9 s in two runs
on the preceding render-prune branch. Last-window input-to-draw p95 remained
about 32 ms, post-GC PSS about 187 MiB, and charged writes about 38.5 MB.
This suggests a possible small sustained-drawing CPU/latency cost; the runs
do not isolate its cause. An isolated empty-preview fast-path experiment did
not recover that difference, so the larger repeated object-creation gain is
retained. The raw stroke run is under
`.test-artifacts/hillclimb/tablet-coalesced-preview-strokes-1000-1/`.

Ten process-cold launches after two warmups on this branch measured 1,333 ms
Android launch median, 2,420 ms process CPU median, 152.8 MiB sampled PSS
median, and about 1.90 MB logical reads. The earlier same-fixture parent run
measured 1,352 ms, 2,420 ms, 152.1 MiB, and about 1.90 MB respectively.
That comparison does not establish a cold-start or memory change. Raw cold
metrics are under `.test-artifacts/hillclimb/tablet-coalesced-preview-cold-1/`.
