# Direct layer scan for erase — NoteAir6C, 2026-10-20

Erasing previously built a temporary item index and sorted the selected
layer's items on every edit. The erase path now scans the copied model's
stroke and object maps in their existing order, selecting only the requested
layer and page. The hit scan runs before action construction; persisted action
order and undo semantics remain unchanged.

The physical NoteAir6C comparison used the same copied note, eight valid
targets per run, and two runs per mode and branch. Each operation was undone;
all runs restored the source geometry and visibility. The isolated test app
was removed after each run. The table reports pooled medians in milliseconds
and the slowest sample from each eight-sample run, since an eight-sample p95
is effectively the run maximum.

| Mode | Branch | Geometry median | Feedback median | Updated draw median | Updated draw run maxima |
| --- | --- | ---: | ---: | ---: | ---: |
| Partial | `perf/settled-stroke-items` | 20.9 | 20.1 | 89.3 | 159.5, 110.1 |
| Partial | `perf/erase-direct-layer-scan` | 17.3 | 23.0 | 85.5 | 126.5, 129.2 |
| Whole item | `perf/settled-stroke-items` | 13.6 | 43.0 | 88.0 | 111.8, 122.8 |
| Whole item | `perf/erase-direct-layer-scan` | 10.1 | 32.0 | 85.4 | 102.0, 101.6 |

The direct scan lowers median geometry time in both modes. Whole-item updated
draw tails improved in both paired runs. Partial updated draw is variable:
the first preferred run had a 159.5 ms outlier, while its second run was
faster than either candidate run. These small sequential samples do not
establish a partial end-to-end tail improvement. The direct scan still walks
all strokes and objects to find one layer; a document with many pages or
layers needs separate scaling measurements.

The final candidate passed the CLI drawing and coverage checks (11 each),
the CLI and proxy suites (22 and 15 checks), and the Android build in the Nix
shell. Raw copied note and timing records stay under `.test-artifacts/`.
