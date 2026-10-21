# Checkpoint JSON load phases — NoteAir6C, 2026-10-21

The current checkpointed `notes` fixture has a 1,215,466-byte base. The
shared journal reader now marks file read and UTF-8 decode separately before
the existing parse mark. On the physical NoteAir6C, ten measured process-cold
launches after two warmups had a 1,428.5 ms Android launch median, 2,555 ms
process CPU median, 157.1 MiB post-launch PSS median, and about 1.89 MB of
logical process reads.

| Base-load segment | Physical median |
| --- | ---: |
| Document scan to base read complete, including journal access setup | 2.9 ms |
| UTF-8 decode | 3.5 ms |
| Strict JSON parse | 389.6 ms |
| Checkpoint stroke-prefix expansion | 5.3 ms |

The read segment includes a small amount of proxy and journal setup, so the
file read itself may be shorter. This fixture is storage-warm; physical disk
read counters vary with cache state. JSON parsing is the dominant measured
base-load cost and the next optimization target. The copied note remained in
the isolated test package; personal app data was untouched. Raw results are
private under `.test-artifacts/hillclimb/tablet-checkpoint-json-phases-1/`.
