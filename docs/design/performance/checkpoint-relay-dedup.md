# Reconstructable relay omitted from checkpoint — NoteAir6C, 2026-10-20

The copied note's checkpointed base held 389 committed operations twice:
`events` and `relay` had the same complete values and order. Each list occupied
about 1.17 MiB of JSON. During checkpoint, the journal now stores an empty
relay only when it is exactly equal to `events` followed by `pending`. The
existing reader reconstructs that list. A relay containing any extra or
reordered ingress entries remains stored in full. The in-memory state and CLI
`log` output remain unchanged.

An actual CLI checkpoint of the copied note preserved the complete `log`
result and revision 847, wrote an empty relay to the base, and restored all
389 relay entries on read. Base size fell from **2,391,204 to 1,216,522
bytes**, with an empty WAL in both cases. A proxy test also verified that a
non-reconstructable relay survives checkpoint and restart.

| Process-cold median, physical NoteAir6C | Full relay run 1 | Full relay run 2 | Omitted relay run 1 | Omitted relay run 2 | Android background checkpoint |
| --- | ---: | ---: | ---: | ---: | ---: |
| Android launch | 1,171 ms | 1,165 ms | 1,094 ms | 1,067 ms | 1,120 ms |
| Base read/decode/parse | 486 ms | 486 ms | 401 ms | 394 ms | 423 ms |
| Process CPU | 2.15 s | 2.16 s | 2.10 s | 2.05 s | 2.12 s |
| Post-launch PSS | 166 MiB | 163 MiB | 151 MiB | 150 MiB | 151 MiB |
| Logical process reads | 2.92 MiB | 2.92 MiB | 1.80 MiB | 1.80 MiB | 1.80 MiB |

Each run had 12 `LaunchState: COLD` opens with two warmups excluded. The first
deduplicated run had one 1,284 ms outlier, giving a p95 above its median;
the second run's p95 was 1,131 ms. Both median and base-parse time improved
repeatedly. The Android workflow started with the original 4.6 MiB WAL,
sent the editor home, observed the WAL reach zero, and confirmed the resulting
base was exactly 1,216,522 bytes before the cold launches. No personal note
was changed.

Two physical checkpoint calls with the deduplicated format wrote about
1.24 MiB each and took 199 and 238 ms wall time. The earlier full-relay
checkpoint calls wrote about 2.42 MiB and took 298 and 305 ms. The wall-time
comparison includes the intervening JSON parser optimization, so it cannot
isolate the checkpoint-format effect; the charged-write reduction follows
directly from the smaller base. Startup runs on both sides used the same fast
parser. These are storage-warm starts, not E Ink scanout measurements.

The CLI and proxy suites passed, including restart, actor history, crash
recovery, and both relay cases. Note-free aggregates and APK hashes are in
[`checkpoint-relay-dedup.json`](checkpoint-relay-dedup.json); raw copied-note
artifacts remain private under `.test-artifacts/hillclimb/`.
