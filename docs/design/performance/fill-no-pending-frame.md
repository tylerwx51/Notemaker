# Defer the pending fill refresh — NoteAir6C, 2026-10-21

`fillAt` previously refreshed the whole editor and Canvas before queuing its
local durable fill command. The Canvas had no new pixels to show at that point.
The Android gesture now waits for the result before its Canvas refresh. If the
command is still pending after 150 ms, it refreshes only the editor chrome so
the saving state remains visible for slower fills. The shared replica command,
offline behavior, atomic history entry, and failure refresh are unchanged.

The isolated physical-device interaction suite performed 20 consecutive fills
in a disposable rectangle document, with four warmups and 16 measured taps.
It waited for durable objects and updated UI and Canvas draws, then verified
all objects and editor history. Two runs measured the plain no-pending-frame
change; a third measured the final delayed-chrome behavior.

| Fill-mode measure | Preferred branch, 3 runs | No pending, 2 runs | Delayed chrome, 1 run |
| --- | ---: | ---: | ---: |
| Input to updated UI draw p95 | 63.1–78.8 ms | 64.9 / 64.9 ms | 65.9 ms |
| Input to updated Canvas draw p95 | 78.9–108.2 ms | 87.7 / 83.2 ms | 86.6 ms |
| Window frames, 20 fills | 41 each | 20 / 20 | 21 |
| Mode process CPU | 1.87–2.17 s | 1.87 / 1.87 s | 1.92 s |
| Charged writes, 20 fills | 352 KiB each | 352 / 352 KiB | 352 KiB |

The reduction in window frames is consistent and material for an E Ink
display. Updated-draw latency remains within the preferred branch's observed
range, so these runs do not establish a latency improvement. The delayed-chrome
run had one extra window frame, consistent with one fill exceeding the delay.
Android timings exclude physical touch sampling and E Ink scanout. Raw metrics
and APK hashes are in `.test-artifacts/hillclimb/tablet-fill-{squared-parent-1,merge-vertical-runs-{1,2},no-pending-frame-{1,2},delayed-pending-1}/`.

The full Android drawing suite passed on this branch, including tap-to-fill,
history, coverage, and panels. Two earlier full runs stopped at an
object-preview frame-count assertion before reaching fill; a focused coverage
rerun and a full parent-branch run passed that assertion. The final full
candidate run passed. This intermittent assertion does not establish a fill
regression, but its cause remains unverified.
