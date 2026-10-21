# Reordered relay encoded as IDs — NoteAir6C, 2026-10-20

A later copied note has 548 relay entries containing the same operations as
`events` and `pending`, but in a different order. The previous checkpoint
preserved the order by writing every relay operation again, duplicating about
2.40 MiB of geometry in the base. A checkpoint now writes `relayFormat: "ids"`
and the relay's ordered operation IDs when those IDs uniquely identify exactly
the operations in `events` and `pending`. The reader expands the IDs before
replaying any WAL records. It rejects missing, duplicate, or unknown IDs.
Exact-order relay omission and full storage of relay with extra or different
entries remain in place. In-memory state and CLI `log` output are unchanged.

An actual CLI checkpoint on a temporary copy reduced the base from **5,114,942
to 2,738,803 bytes** and cleared the 6,275,864-byte WAL. `log` matched before
and after checkpoint and after proxy restart. The revision remained 879. The
proxy and CLI suites passed, including the new reordered-relay round trip.
The full Android emulator drawing suite also passed on the candidate.

| Physical NoteAir6C, checkpointed note | Full relay run 1 | Full relay run 2 | ID order run 1 | ID order run 2 |
| --- | ---: | ---: | ---: | ---: |
| Process-cold launch median | 1,905 ms | 1,922 ms | 1,737 ms | 1,755 ms |
| Launch maximum | 1,938 ms | 1,972 ms | 3,592 ms | 1,861 ms |
| Process CPU median | 3,060 ms | 3,055 ms | 2,870 ms | 2,875 ms |
| Post-launch PSS median | 228.25 MiB | 228.32 MiB | 191.93 MiB | 191.95 MiB |
| Logical process reads median | 5.518 MiB | 5.518 MiB | 3.253 MiB | 3.253 MiB |

Each run has 12 `LaunchState: COLD` opens with two warmups excluded. The first
ID-order run had one 3,592 ms outlier; its median still improved, and the repeat
run did not reproduce the outlier. These are storage-warm Android launch
measurements, not E Ink scanout measurements.

| Physical checkpoint call | Full relay run 1 | Full relay run 2 | ID order run 1 | ID order run 2 |
| --- | ---: | ---: | ---: | ---: |
| Wall time | 524 ms | 531 ms | 334 ms | 359 ms |
| Process CPU | 619 ms | 646 ms | 520 ms | 611 ms |
| Charged writes | 5,152,768 B | 5,148,672 B | 2,764,800 B | 2,764,800 B |

These runs used the same device, fixture, and test harness on parent and
candidate branches. A no-op second checkpoint took about 1.6–1.9 ms and wrote
zero bytes in all four runs. The ID format changes the local base schema:
older app or CLI binaries cannot read an ID-order checkpoint. Keep app and CLI
at the same version when sharing a replica or rolling back. The test used a
temporary copy; no personal note was changed. Raw fixture and device outputs
remain private under `.test-artifacts/hillclimb/`.
