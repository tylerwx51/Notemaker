# Precomputed selection transforms — NoteAir6C, 2026-10-21

Object hit testing transformed each selection-region vertex with repeated map
lookups and sine/cosine calls. This branch calculates the object transform once
per candidate and applies it to all vertices. The existing coverage test,
stroke path, and selection lease protocol remain unchanged.

The copied `Drawing object protocol.` fixture was exercised on the physical
NoteAir6C in the isolated interaction suite: 35 measured selection taps after
five warmups in each run. The preferred-parent run was taken after the two
candidate runs on the same device. Earlier current-note runs provide a second
parent comparison. Values below are median / p95 milliseconds.

| Selection metric | Earlier parent runs | Fresh parent | Candidate run 1 | Candidate run 2 |
| --- | ---: | ---: | ---: | ---: |
| Hit-test action | 22.4 / 25.7; 21.6 / 26.1 | 22.0 / 26.4 | 19.7 / 22.8 | 19.8 / 22.6 |
| Input to first updated UI draw | 41.8 / 62.9; 40.8 / 67.3 | 42.5 / 65.0 | 42.9 / 66.2 | 43.4 / 64.4 |
| Input to updated Canvas draw, p95 | 75.1; 79.4 | 78.0 | 76.4 | 76.8 |

Forty selection acquire/release cycles consumed 5.11 seconds of process CPU
on the fresh parent and 4.79/4.81 seconds on the candidate. All three runs
charged zero writes during this mode. Sampled PSS before/after was about
165/181 MiB in all runs. The repeated action-phase reduction does not produce
a resolved first-draw latency improvement. Other interaction-mode p95 results
stayed close to their prior ranges.

The candidate passed 22 CLI checks, 17 proxy checks, the Android focused
coverage check, and the full Android drawing suite. Retain this branch as the
new preferred performance revision for its lower selection CPU and action
time, while treating first-draw latency as unchanged. Raw private traces and
APK hashes are under
`.test-artifacts/hillclimb/tablet-selection-transform-{precompute-{1,2},parent-current-1}/`.
These timings exclude physical touch sampling and E Ink scanout.
