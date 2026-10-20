# Physical object history action sweep — 2026-10-20

The isolated NoteAir6C harness now alternates 60 undo and redo commands for a
single rectangle added to the copied note through the local replica. Six actions
warm the mode; each direction has 27 measured actions. A sample ends only after
the model has the expected visibility and the window or Canvas draws. The test
also checks that the final object and editor history survive in the local
replica. The installed app and original note are untouched.

| Action | First updated window draw median | p95, two runs | Updated Canvas draw median |
| --- | ---: | ---: | ---: |
| Undo object | 65.0–65.3 ms | 89.9–92.6 ms | 69.1–70.7 ms |
| Redo object | 68.1–68.9 ms | 72.1–95.5 ms | 77.1–78.0 ms |

The action handler itself has a 0.8–1.3 ms median across both directions. It
only schedules the local history command; the much longer handler-to-draw wait
includes the replica write, snapshot refresh, and rendering. Per-frame command
issue has a 49.2–49.8 ms p95 across all frames in this mode, but frame samples
are not paired one-for-one with actions. The device charged exactly 1,146,880
bytes of writes per 60-action run (about 19 KiB per toggle), with 5.9–6.2 s of
process CPU time. Sampled PSS fell from about 195 to 183 MiB during the mode;
after explicit GC it was about 144 MiB. The PSS change reflects collection of
earlier allocations and does not establish a history memory saving.

The note-free timings, resource counters, and APK hashes for both physical runs
are in [`history-action-sweep.json`](history-action-sweep.json). Input is queued
synthetically within the app, so physical touch sampling and E Ink scanout are
outside these measurements. The original captured note and raw traces remain
private under `.test-artifacts/hillclimb/`.
