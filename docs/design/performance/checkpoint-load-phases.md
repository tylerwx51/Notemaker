# Checkpointed base load phases — NoteAir6C, 2026-10-20

The isolated NoteAir6C package opened the copied 2.39 MiB checkpointed base
12 times with an offline document index. Two cold-launch warmups were excluded.
Android launch median was 1,388 ms, matching earlier checkpointed runs.

| Root-validation segment | Median |
| --- | ---: |
| Read, decode, and parse base JSON | 724 ms |
| Prepare relay IDs and metadata | 0.6 ms |
| Apply 389 committed events | 10 ms |
| Build effective model | 0.5 ms |
| Finish root validation | 0.6 ms |

The first segment includes the cached filesystem read and UTF-8 decoding as
well as the strict JSON parser; these have not been split yet. The device
charged essentially no physical disk reads in the prior checkpointed launch
runs, so parsing is the likely dominant cost. Saving a second materialized
model in the base would target the 10 ms event replay rather than the 724 ms
segment. The next experiment is a faster strict JSON parser on the same
checkpointed fixture. Note-free metrics are in
[`checkpoint-load-phases.json`](checkpoint-load-phases.json).
