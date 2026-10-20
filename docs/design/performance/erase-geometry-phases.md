# Erase geometry phases — NoteAir6C, 2026-10-20

The isolated physical tablet used the same copied note and eight valid
targets per erase mode. Each operation was undone, and both runs verified
that source geometry and visibility were restored. The measurement branch
separates hit detection from result construction without changing output
order or persisted actions. CLI and proxy suites passed (22 and 15 checks).

| Geometry phase | Partial median / max | Whole-item median / max |
| --- | ---: | ---: |
| Construct `LayerEditor` | 10.1 / 24.5 ms | 9.2 / 15.1 ms |
| Build eraser regions | 0.5 / 0.8 ms | 0.4 / 0.6 ms |
| Scan candidate coverage | 3.6 / 12.9 ms | 2.2 / 5.6 ms |
| Build replacement actions | 1.8 / 9.3 ms | <0.1 / <0.1 ms |
| Validate replacement actions | 1.8 / 5.5 ms | <0.1 / 0.2 ms |

The eight-sample input-to-updated-draw p95 was 123.4 ms for partial erase
and 130.2 ms for whole-item erase. Input-to-preview p95 was 26.8 and
74.4 ms respectively. The layer editor constructs an item index on the
newly copied model used by each command; that repeated setup is the next
target. Samples are sequential and small, so these p95 values equal the
slowest action. Raw note and timing records stay private under
`.test-artifacts/hillclimb/`.
