# Background checkpoint on NoteAir6C — 2026-10-20

The selected local proxy's manual checkpoint folded a copied 4,810,638-byte
journal into a 2,391,204-byte base on the physical NoteAir6C. Two fresh
isolated-package runs measured **298 and 305 ms wall time**, **470 and 477 ms
process CPU**, and **2.42 MiB charged writes**. The second no-op checkpoint in
each run took 1.6–1.7 ms and wrote no bytes. The process's sampled PSS fell
after the checkpoint, but this is not a peak-memory measure. The host and the
installed personal app's note were untouched.

Android now queues a checkpoint after `MainActivity.onStop` when the local
document WAL is at least 1 MiB. Queued saves run first, and a worker skips the
checkpoint if the Activity has already returned to the foreground. The queue
call and file-size check are the only work on the UI thread. An interrupted
checkpoint retains the journal's existing crash-recovery behavior. The
isolated-package harness verified that the WAL was still large while the
editor was visible, then sent it home and observed the WAL reach zero.

| Process-cold metric | Original WAL | Background checkpoint run 1 | Background checkpoint run 2 |
| --- | ---: | ---: | ---: |
| Android launch median | 1,660 ms | 1,443 ms | 1,416 ms |
| Android launch p95 | 1,698 ms | 1,500 ms | 1,531 ms |
| Process CPU median | 2.67 s | 2.45 s | 2.39 s |
| Post-launch PSS median | 200 MiB | 171 MiB | 171 MiB |

Each launch run contained 12 `LaunchState: COLD` samples; two were excluded as
warmups. All used the same source APK and a disposable offline proxy index for
the copied document. The background checkpoint improved median launch time by
217–244 ms (13–15%) on this storage-warm fixture. The 1.5 s from the HOME
input command to *observing* an empty WAL includes shell invocation and polling
over ADB; it is not the checkpoint's execution time.

This scheduling favors edit latency over immediate compaction. A user who
keeps the editor visible will retain the WAL until it is hidden or they invoke
the CLI command. If Android kills the background process before the queued
task finishes, a later launch replays the WAL. The benchmark does not measure
E Ink scanout or the responsiveness of another app during the background CPU
work. Note-free metrics and APK hashes are in
[`checkpoint-scheduling.json`](checkpoint-scheduling.json); raw artifacts stay
under `.test-artifacts/hillclimb/`.
