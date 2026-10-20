# Physical process-cold editor start — 2026-10-20

The isolated NoteAir6C harness copied the captured note into a separate app
package, then force-stopped and launched that package 12 times per run. It
excluded two warmups and required Android to report `LaunchState: COLD` on
every sample. `am start -W` supplied Android's launch `TotalTime`; per-process
CPU and I/O were read from `/proc`, and PSS from `dumpsys meminfo` immediately
after launch. CPU time sums work across process threads, so it can exceed wall
time. The installed NoteMaker app and its note were untouched.

| Per-run metric | Default compilation, three runs | Forced `speed` compilation, two runs |
| --- | ---: | ---: |
| Android launch median | 1,636–1,657 ms | 1,612–1,646 ms |
| Android launch range across measured samples | 1,605–1,701 ms | 1,579–1,711 ms |
| Host `am start -W` wall median | 1,730–1,749 ms | 1,694–1,742 ms |
| Process CPU median | 2.65–2.68 s | 2.61–2.65 s |
| Post-launch PSS median | about 193 MiB | 196–199 MiB |
| Logical process reads median | 5.22 MiB | 4.81 MiB |

Default-compilation runs charged no disk reads in the measured launches and
about 32 KiB of writes each. The copied note and executable remained in the
device page cache, so these are process-cold, storage-warm starts. Forced
ahead-of-time compilation returned `Success`, but its launch-time ranges
overlap the default runs; this experiment does not establish a reliable speed
gain from changing compilation mode. Sampled PSS was slightly higher with
forced compilation, but the runs do not establish a stable memory cost.

The note-free per-launch values, APK hashes, and aggregate counters are in
[`cold-start-baseline.json`](cold-start-baseline.json). Android's launch time is
not physical finger-to-ink latency or E Ink scanout. App and OS startup phases
remain to be attributed before choosing an implementation change.
