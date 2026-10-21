# Current checkpointed cold start — NoteAir6C, 2026-10-21

Branch `perf/current-cold-start` reopened the same copied `notes` replica used
for the relay-ID checkpoint study. The isolated Android package started with a
376 KiB base and a 6.0 MiB WAL, backgrounded the editor until its checkpoint
completed, then force-stopped and process-cold-launched the app 12 times.
Two warmups were excluded. The resulting base was 2,738,803 bytes in all runs.

| Median per run | Prior relay-ID run 1 | Prior run 2 | Current build |
| --- | ---: | ---: | ---: |
| Android launch | 1,737 ms | 1,755 ms | 1,730 ms |
| Process CPU | 2,870 ms | 2,875 ms | 2,840 ms |
| Post-launch PSS | 191.9 MiB | 192.0 MiB | 190.4 MiB |
| Logical reads | 3.253 MiB | 3.253 MiB | 3.255 MiB |

The launch time is stable across the later rendering and selection changes.
Current-run charged reads had a 34 KiB median versus zero in the prior runs;
that small cache-state difference does not establish an I/O regression. Charged
writes were 32 KiB per launch in all runs. The current run's longest measured
launch was 1,769 ms; one prior run had an isolated 3,592 ms outlier.

An actual CLI checkpoint on a separate copy produced the same 2,738,803-byte
base. It contains 548 committed operations, including 474 stroke snapshots.
Of 2.12 MiB of point-array JSON, about 1.53 MiB repeats exact prefixes from
earlier snapshots of the same stroke. This is a storage and parse-cost target;
encoding and expanding those prefixes may have its own CPU cost. The analysis
only prints aggregate sizes and remains under `.test-artifacts/hillclimb/`.

These are storage-warm Android launch and process counters, not E Ink scanout
or a cold physical-disk read. The copied note and raw traces remain private in
`tablet-current-cold-checkpoint-1/` and the earlier relay-ID run directories.
