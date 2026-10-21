# Merge vertically adjacent fill runs — NoteAir6C, 2026-10-21

`RegionFill.create` represented every one-pixel-high filled scanline run as a
separate SVG rectangle. This branch carries a run forward when the next row
has the same horizontal span, emitting one taller rectangle when that span
ends. The bounded flood, covered pixels, color, action type, and shared local
replica path are unchanged. Existing stored fills still render as before.

The isolated NoteAir6C interaction suite performed 20 consecutive fills in a
disposable rectangle document, with four warmups and 16 measured taps. It
waited for each durable SVG object and updated UI and Canvas draws, then
verified all objects and editor history. Two candidate runs used the same
fixture as a fresh parent run and two recent parent runs.

| Fill-mode measure | Parent range, 3 runs | Candidate 1 / 2 |
| --- | ---: | ---: |
| Input to updated UI draw p95 | 59.2–88.7 ms | 63.1 / 78.8 ms |
| Input to updated Canvas draw p95 | 88.8–146.3 ms | 78.9 / 108.2 ms |
| Window draw phase p95 | 38.1–43.5 ms | 15.8 / 23.3 ms |
| Mode process CPU | 2.03–2.33 s | 1.87 / 2.17 s |
| Charged writes, 20 fills | 488 KiB in each run | 352 / 352 KiB |
| Java heap used after mode | 54.5–55.5 MiB | 53.1 / 52.1 MiB |

The draw phase and charged writes improved consistently. The first-draw p95
also improved in both candidate runs relative to the fresh parent, but frame
scheduling caused substantial variation among parents. Process CPU is lower
in both candidate runs than the fresh parent; comparisons with older parents
are less decisive. PSS and heap samples are sensitive to collection timing
and do not establish retained-memory savings.

Actual CLI calls produced rectangular and elliptical fills on disposable
replicas. Their generated SVG sources shrank from 3,626 to 163 bytes and from
3,588 to 2,329 bytes respectively. Exported PNG files were byte-identical
between parent and candidate for both cases. The shared CLI drawing and
coverage checks passed (14 and 11 checks), as did the full Android drawing
suite, including fill-tap persistence and pixel coverage. Retain this branch
as the preferred performance build. Raw physical metrics and APK hashes are
under `.test-artifacts/hillclimb/tablet-fill-merge-{parent-1,vertical-runs-{1,2}}/`;
the CLI pixel-comparison files remain private under
`.test-artifacts/hillclimb/fill-merge-parity/`. Android timings exclude
physical touch sampling and E Ink scanout.
