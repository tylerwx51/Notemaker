# Stroke point prefixes in checkpoints — NoteAir6C, 2026-10-21

The copied `notes` fixture has 548 events, including 474 snapshots of strokes.
About 1.53 MB of its 2.12 MB of point-array JSON is an exact prefix of an
earlier snapshot of the same stroke. A checkpoint now records one prefix count
per stroke action and stores only the remaining point rows. It walks events,
pending actions, and nested batches in order. Prefixes shorter than eight
points remain complete. The decoder validates the reference and reconstructs
full actions before relay expansion or model replay. The WAL, live actions,
and CLI `log` output retain full point lists.

An actual CLI checkpoint on a temporary copy reduced the base from **2,738,803
to 1,215,466 bytes**, a 55.6% reduction, and cleared the 6,275,864-byte WAL.
`log` matched before and after checkpoint and after proxy restart. A further
append and restart preserved the complete point list. A malformed prefix
reference was rejected. The format keeps the relay ID ordering from the
preceding checkpoint work.

| Physical NoteAir6C, checkpointed note | Prior format | Prefix run 1 | Prefix run 2 |
| --- | ---: | ---: | ---: |
| Process-cold launch median | 1,730 ms | 1,591.5 ms | 1,586.5 ms |
| Launch maximum | 1,769 ms | 1,631 ms | 1,693 ms |
| Process CPU median | 2,840 ms | 2,690 ms | 2,740 ms |
| Post-launch PSS median | 190.4 MiB | 169.1 MiB | 169.1 MiB |
| Logical process reads median | 3.25 MiB | 1.80 MiB | 1.80 MiB |

Each run used the same fixture and 12 `LaunchState: COLD` opens, excluding two
warmups. A background checkpoint produced the base before measuring launches.
These are storage-warm Android launch measurements; physical E Ink scanout was
not measured. The prior-format run used the immediately preceding code, and
the two prefix runs repeated the candidate on the same device.

| Physical checkpoint call | Prior format run 1 | Prior format run 2 | Prefix run 1 | Prefix run 2 |
| --- | ---: | ---: | ---: | ---: |
| Wall time | 334 ms | 359 ms | 183 ms | 193 ms |
| Process CPU | 520 ms | 611 ms | 238 ms | 345 ms |
| Charged writes | 2,764,800 B | 2,764,800 B | 1,236,992 B | 1,236,992 B |

The checkpoint-call prior-format runs came from the earlier relay-ID branch,
with the same device, fixture, and harness. A no-op second checkpoint wrote
zero bytes and took about 1.6–2.1 ms across the runs. The proxy, CLI, and
selection suites passed, as did the full Android emulator drawing suite.

`stroke-prefix-v1` changes the local base schema. Older app or CLI binaries
cannot read a checkpoint using it; keep app and CLI at the same version when
sharing a replica or rolling back. The tests used a temporary copied note; no
personal note was changed. Raw fixture and device outputs remain private under
`.test-artifacts/hillclimb/`.
