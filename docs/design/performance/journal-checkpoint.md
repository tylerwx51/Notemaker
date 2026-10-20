# Local journal checkpoint — physical experiment, 2026-10-20

The isolated NoteAir6C package opened the same captured note from two local
replica layouts. The original had a 111-byte base and a 4,810,638-byte WAL
(847 records). An actual CLI `checkpoint` call on a copy preserved the complete
`log` result and revision 847, producing a 2,391,204-byte base and empty WAL.
That host-side call took 125 ms wall time; it is not a physical-device
checkpoint cost. The installed personal app and note were untouched.

Each physical run had 12 Android process-cold launches, with two warmups
excluded and `LaunchState: COLD` required. The offline proxy config was seeded
with only the copied document name and fresh credentials, so known-document
discovery did not parse the large base twice. The original-WAL comparison used
the same source build and setup as the two checkpointed runs.

| Per-run median | Original WAL | Checkpointed run 1 | Checkpointed run 2 |
| --- | ---: | ---: | ---: |
| Android launch | 1,653 ms | 1,385 ms | 1,401 ms |
| Android launch p95 | 1,710 ms | 1,435 ms | 1,446 ms |
| Process CPU, all threads | 2.66 s | 2.36 s | 2.40 s |
| Post-launch PSS | 199 MiB | 170 MiB | 166 MiB |
| Logical process reads | 5.22 MiB | 2.91 MiB | 2.91 MiB |
| Charged disk writes | 32 KiB | 32 KiB | 32 KiB |

The checkpointed launches were 252–268 ms faster at the median (15–16%),
with about 260 ms earlier first view draw in the phase trace. Root validation
fell from about 981 ms to 711–723 ms. The checkpointed runs also used about
260–300 ms less process CPU and sampled less memory; PSS is a post-launch
sample, not peak memory. Charged disk reads were near zero in all runs because
the copied files remained in the device page cache. These numbers measure
process-cold, storage-warm startup, not E Ink scanout.

Without a known-document index, checkpointing *regressed* startup: 1,801 ms
median. Discovery parsed the full base, then root validation reconstructed the
model. The proxy now skips discovery parsing for names already in its local
config and still validates each document during `Store.read`. Newly discovered
documents still pay the discovery parse on that first proxy start.

The checkpoint is manual on this branch. Automatic timing and physical-device
checkpoint I/O have not been measured. Proxy and CLI checks passed, including
actor undo/redo after restart and a simulated crash between base replacement
and WAL truncation. The note-free aggregate values and APK hashes are in
[`journal-checkpoint.json`](journal-checkpoint.json); raw copied-note artifacts
remain private under `.test-artifacts/hillclimb/`.
