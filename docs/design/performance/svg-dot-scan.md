# Generated dot-grid syntax scan — NoteAir6C, 2026-10-21

The previous `perf/svg-template-render` branch validates NoteMaker's generated
dot-grid SVG without building an XML DOM. Its strict `<circle>` matcher and
numeric checks still took 44–51 ms for the 1,333-circle copied note. This
branch scans that fixed syntax directly, validates each decimal and the exact
tag sequence, and keeps the same general-XML fallback for other SVGs. SVG
normalization and AndroidSVG rendering remain unchanged.

| Physical NoteAir6C, checkpointed note | Regex run 1 | Regex run 2 | Scan run 1 | Scan run 2 | Regex bracket |
| --- | ---: | ---: | ---: | ---: | ---: |
| Process-cold launch median | 1,466 ms | 1,473 ms | 1,434.5 ms | 1,455 ms | 1,484.5 ms |
| Process CPU median | 2,575 ms | 2,560 ms | 2,540 ms | 2,575 ms | 2,575 ms |
| Post-launch PSS median | 158.1 MiB | 157.8 MiB | 156.8 MiB | 157.1 MiB | 157.3 MiB |
| Logical process reads median | 1,892,842 B | 1,892,893 B | 1,893,112 B | 1,893,112 B | 1,892,893 B |
| SVG validation median | 51.4 ms | 44.3 ms | 36.0 ms | 36.2 ms | 44.7 ms |

Each physical run used the same device and copied fixture, with 12
process-cold launches and two warmups excluded. The final regex run brackets
the candidate and remained slower at 1,484.5 ms. The validation segment is
8–15 ms faster than the regex runs; whole-launch medians improved by 11–50 ms,
but device variation contributes to that larger range. Android's 1,333-dot
pixel comparison and the 22 CLI checks passed. An actual offline CLI call
generated a 1,333-dot template as one SVG object. The scan does not alter
stored actions, render output, or physical E Ink scanout behavior.

A separate direct-Canvas rendering experiment on this branch was rejected
before these measurements: rendering 1,333 circles as Canvas paths changed
6,731 pixels against AndroidSVG, and a combined fill/stroke variant changed
5,312. The direct renderer was removed. The temporary copied note did not
change personal app data. Raw logs remain private under
`.test-artifacts/hillclimb/tablet-svg-dot-scan-*`.
