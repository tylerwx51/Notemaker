# Refresh chrome only for idle tool switches — NoteAir6C, 2026-10-20

Selecting a drawing tool previously invalidated the full canvas even when the
page pixels were unchanged. `setDrawingTool` now refreshes the toolbar alone
when no shape drag is visible. If a tool switch cancels a visible shape preview,
it still redraws the canvas so the preview disappears. The app keeps a normal
window frame for the changed toolbar state.

Two isolated physical NoteAir6C interaction runs used the same copied `notes`
document and 40 alternating tool selections, with five warmups. The benchmark
records first updated window draw for this mode; no canvas draw is expected.

| Measure | Parent runs | Chrome-only runs |
| --- | ---: | ---: |
| Input to updated window draw p95 | 29.19 / 29.16 ms | 25.28 / 25.40 ms |
| Measured Canvas draws | 35 / 35 | 0 / 0 |
| Total mode window frames | 41 / 41 | 41 / 41 |
| Per-frame draw p95 | 22.14 / 22.91 ms | 6.64 / 6.71 ms |
| Process CPU | 1.67 / 1.72 s | 1.12 / 1.11 s |
| Charged writes | 0 / 0 KiB | 0 / 0 KiB |
| Sampled PSS after mode | 167 / 167 MiB | 166 / 166 MiB |

The change removes 35 unnecessary canvas draws per measured run and saves
about 4 ms at the first updated window draw. RAM and I/O are effectively
unchanged. The timings stop at Android app/window draw before physical E Ink
scanout. Raw metrics and APK hashes are under
`.test-artifacts/hillclimb/tablet-tool-switch-chrome-only-{1,2}/` and
`.test-artifacts/hillclimb/tablet-object-create-direct-snapshot-{1,2}/`.
