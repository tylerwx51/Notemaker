# Physical cold-start phase attribution — 2026-10-20

The `perf/cold-start-phases` branch logged on-device milestones for 12
process-cold opens of the same copied NoteAir6C note. The log observer is active
only in the isolated latency package. Android still reported a roughly 1.6 s
launch, consistent with the uninstrumented baseline.

| Phase | Median elapsed from `onCreate` start |
| --- | ---: |
| Preferences read | 6 ms |
| Replica object created | 8 ms |
| First snapshot returned | 1,002 ms |
| Android model refreshed | 1,024 ms |
| Editor view built | 1,049 ms |
| First view draw began | 1,124 ms |
| Post-draw callback ran | 1,383 ms |

The first snapshot call spent about 991 ms in `ReplicaService.ensure()` and
about 2 ms executing the snapshot command. Inside the local proxy constructor,
document discovery finished at about 5 ms; validation of each document's root
identity finished at about 975 ms. The **root-validation read consumed about
970 ms median**. In this fixture, the authoritative base file is tiny while its
journal is about 4.6 MiB with 847 records, so the first `Store.read()` replays
the journal. Restoring saved UI preferences took under 1 ms after the snapshot,
and refreshing the Android model took about 23 ms.

These timings identify journal reconstruction as the next optimization target.
Skipping root validation alone would move the reconstruction to the subsequent
snapshot command; it would not make the note ready sooner. The note-free phase
summary and APK hash are in
[`cold-start-phases.json`](cold-start-phases.json). Raw logcat and the copied
note remain private under `.test-artifacts/hillclimb/`.
