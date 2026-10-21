# Generated dot-grid SVG validation — NoteAir6C, 2026-10-21

A copied `notes` fixture contains a fixed-page dot-grid template with 1,333
`<circle>` elements in 122,125 bytes of SVG. First-draw tracing attributed
about 412 ms to its first render. Splitting the renderer showed that shared
SVG validation and DOM normalization took about 264 ms, AndroidSVG parsing
about 102 ms, and picture recording about 41 ms.

The shared `SvgAsset` now recognizes only the exact, self-contained SVG grammar
emitted by NoteMaker's dot-grid template action. It checks root dimensions,
stroke properties, every circle, the element count, and the complete string,
then supplies the same percentage-size viewport wrapper directly. Any other
SVG goes through the existing XML validator. Stored actions, CLI output, and
the general SVG profile remain unchanged.

| Physical NoteAir6C, checkpointed copied note | Prior validator | Fast run 1 | Fast run 2 |
| --- | ---: | ---: | ---: |
| Process-cold launch median | 1,614 ms | 1,466 ms | 1,473 ms |
| Launch maximum | 1,675 ms | 1,530 ms | 1,519 ms |
| Process CPU median | 2,755 ms | 2,575 ms | 2,560 ms |
| Post-launch PSS median | 172.3 MiB | 158.1 MiB | 157.8 MiB |
| Logical process reads median | 1,892,002 B | 1,892,842 B | 1,892,893 B |
| SVG validation and normalization median | 264 ms | 51 ms | 44 ms |
| AndroidSVG parse median | 102 ms | 106 ms | 102 ms |

Each physical run used the same device, copied fixture, and 12 process-cold
launches with two warmups excluded. The repeated gain is about 141–148 ms at
the launch median. The PSS difference is sampled after launch and includes
garbage-collector variation, though the two candidate runs were close. A
1,333-circle Android pixel comparison matched the general DOM path exactly;
CLI and proxy suites passed. The full Android drawing suite also passed.

These are storage-warm Android launch measurements, not physical E Ink
scanout. The temporary copied note did not change personal app data. Raw
fixture and device logs remain private under `.test-artifacts/hillclimb/`.
