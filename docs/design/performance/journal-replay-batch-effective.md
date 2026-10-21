# Rebuild the effective model once after journal replay — NoteAir6C, 2026-10-20

At process start, the journal reader previously rebuilt the effective model
after every replayed acknowledgement. It now applies each CRC-checked record
and committed operation in order, then rebuilds the effective model once from
the final committed state and pending operations. Live writes retain the
existing per-transaction update path. Journal format, revision checks,
actor histories, and durability are unchanged.

The copied `notes` fixture has a 376 KiB base and a 6.0 MiB WAL with 689
records. Ninety-six records contain committed events or pending removals and
previously triggered effective-model reconstruction. Two isolated physical
NoteAir6C runs per branch force-stopped and process-cold-launched the app 12
times each, omitting two warmups. Every measured launch reported
`LaunchState: COLD`. A disposable offline proxy index avoided unrelated
document discovery.

| Per-run median | Parent 1 / 2 | Batched replay 1 / 2 |
| --- | ---: | ---: |
| Android launch | 2,029 / 2,054 ms | 2,000 / 2,005 ms |
| Process CPU | 3.32 / 3.36 s | 3.25 / 3.24 s |
| Post-launch PSS | 158.5 / 158.5 MiB | 158.5 / 158.5 MiB |
| Logical process reads | 6.99 / 6.99 MiB | 6.99 / 6.99 MiB |

The median launch gain repeats at about 24–54 ms. The change avoids redundant
model construction during WAL replay, but parsing the 6.0 MiB journal and
Android startup still dominate. CLI and proxy suites passed (22 and 15 cases),
including restart, actor history, checkpoint recovery, and journal corruption
checks. Physical timings are storage-warm and stop at Android launch reporting,
before E Ink scanout. Raw metrics and APK hashes are under
`.test-artifacts/hillclimb/tablet-{cross-page-cold-start,journal-replay-batch-cold}-{1,2}/`.
