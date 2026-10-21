# Avoid the transient object-creation frame — NoteAir6C, 2026-10-20

`persistObject` previously redrew the editor immediately after queuing a new
object, before the object existed in the local replica. It redrew again after
the durable command returned. This branch retains the pending-history guard,
save count, error feedback and result redraw, but omits the first empty frame.
The object action and editor-history save still use the shared local replica.

Two isolated physical NoteAir6C runs used the same copied `notes` document and
40-rectangle sweep as `perf/object-create-sweep`. Five actions warmed each run;
all 40 objects and editor-history entries were checked in the durable replica.

| Measure | Baseline runs | Candidate runs |
| --- | ---: | ---: |
| Input to updated window draw p95 | 68.27 / 66.81 ms | 62.99 / 60.74 ms |
| Input to updated Canvas draw p95 | 83.79 / 79.35 ms | 86.47 / 79.27 ms |
| Measured window frames | 86 / 84 | 43 / 43 |
| Per-frame command-issue p95 | 49.04 / 48.19 ms | 4.11 / 4.49 ms |
| Charged writes | 744 / 744 KiB | 748 / 744 KiB |
| Process CPU | 3.95 / 3.80 s | 3.70 / 3.63 s |

The branch consistently removes about one transient window frame per object
and improves the first updated window draw by 4–8 ms. The Canvas p95 did not
show a repeatable improvement or regression. Write volume is unchanged within
one filesystem page. The test uses synthetic app input and measures Android
draw, not the physical E Ink scanout. Raw metrics and APK hashes are under
`.test-artifacts/hillclimb/tablet-object-create-no-pending-frame-{1,2}/`.
