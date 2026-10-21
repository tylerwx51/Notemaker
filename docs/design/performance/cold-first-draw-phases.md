# First cold draw phases — NoteAir6C, 2026-10-21

The current checkpointed `notes` fixture launched in about 1.59–1.61 seconds
median across isolated NoteAir6C runs. A process-cold trace with first-draw
marks shows that rendering, as well as base parsing, remains on the launch
critical path. The base read/decode/parse segment took about 400 ms. The first
Canvas draw took about 545 ms, followed by about 195 ms before the posted draw
callback. That callback is a UI scheduling marker, not a physical E Ink
scanout measurement.

The first Canvas draw visits four visible layers. A parser originally paired
the final layer's preparation mark with the first layer's start and falsely
attributed 540 ms to preparation. Reading marks in per-layer order gives:

| First-draw phase | Median across process-cold opens |
| --- | ---: |
| Layer 1 tile draw | 442 ms |
| Layer 2 tile draw | 53 ms |
| Layer 3 tile draw | 30 ms |
| Layer 4 tile draw | 6.5 ms |
| Stroke path compilation, all layers | 39 ms |
| Stroke path replay, all layers | 10 ms |
| Non-stroke item rendering, all layers | 457 ms |
| Largest single non-stroke render | 412 ms, SVG |

The SVG-heavy object in this copied note is a generated dot-grid template
containing 1,333 `<circle>` elements in about 122 KB of SVG. The first render
parses it and records an Android `Picture`; subsequent tile uses reuse that
picture. This explains why the first render is expensive, though the trace
does not split SVG parsing from picture recording or rasterization. Skipping
Canvas matrix setup when no erase preview existed was measured separately and
did not reduce the tile phase, so it was reverted.

This branch adds first-draw timing marks and aggregate item timers only to the
isolated `com.notemaker.latency` package. Repeated physical trace runs used the
same copied fixture and ten measured process-cold launches after two warmups;
the final run's launch median was 1,596 ms. Raw logs remain private under
`.test-artifacts/hillclimb/tablet-cold-*`. No personal note was changed.
