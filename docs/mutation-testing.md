# Mutation testing report — 2026-11-07

This report measures how well the automated tests detect injected faults
(mutants), which code is weakly tested, and which tests do the detecting.
The full generated tables, including every surviving mutant, are in
[mutation-testing-results.md](mutation-testing-results.md). The workflow and tooling
decision are in [ADR 0033](adr/0033-mutation-testing.md); the commands are in
the README section "Mutation testing".

## Method

- **Mutants.** `cargo mutants --list` (cargo-mutants 27.1.0, from `shell.nix`)
  generated 16,864 Rust mutants: 6,294 in `notemaker_core`, 1,990 in
  `notemaker_cli` and 8,580 in `notemaker_android`.
  `tools/mutation/python_mutants.py` generated 614 mutants for the Python server
  (`apps/server/server.py`, `ui_sessions.py`, `svg_asset.py`): negated
  comparisons, `and`/`or` swaps, `not` removal, `True`/`False` swaps, deleted
  call statements and early `return None`.
- **Sample.** Each file got evenly spaced picks. Core and CLI: 1 in 12 (at
  least 3 per file), 734 mutants. Server: 1 in 4, 155 mutants. Android: 1 in 10
  (at least 2 per file), 878 mutants. That is 1,767 in total, about 10%.
- **Tests run per mutant.**
  - Core and CLI mutants: `cargo build --release -p notemaker_cli`, then
    `cargo test --workspace` and all 13 suites of the local tier
    (`tests/run.py --tier local`), run concurrently.
  - Server mutants: the 13 local suites.
  - Android mutants: `cargo test -p notemaker_android` only. The emulator tier
    (`integration`, `cli_android`, ... ) takes about 25 minutes per pass and
    needs an APK build, so it was not run per mutant. Android scores below
    therefore measure the **unit tests only**.
- **Attribution.** `tools/mutation/unittest_json.py` records each Python test's
  outcome, and the driver parses `cargo test` output per test. A mutant is
  *caught* if a test that passed in every baseline run fails, if a suite
  crashes, or if a suite hangs past its timeout.
- **Noise control.**
  - 10 unmutated baseline runs ran under the same load. Tests that failed in
    any baseline are excluded.
  - If 3 or fewer tests (or suites) fail, the failing suites run again with the
    mutant still applied. A failure counts only if it repeats. This dropped 69
    failures that did not repeat.
  - Each command runs in its own systemd scope with a 4 GB memory cap
    (see "Operational findings").
- **Cost.** About 6 hours of wall time with 6–7 parallel workers on the
  16-core development machine.

"Score" below means caught / viable, where viable excludes mutants that do not
compile (unviable).

## Headline results

| Package | Score | Caught / viable | Unviable | What ran |
|---|---|---|---|---|
| `server` (Python) | 92% | 142/155 | 0 | local tier |
| `notemaker_core` | 78% | 419/540 | 14 | Rust tests + local tier |
| `notemaker_cli` | 44% | 74/168 | 12 | Rust tests + local tier |
| `notemaker_android` | 18% | 98/537 | 341 | Rust unit tests only |

The scores measure different things, as the "What ran" column shows:

- **Core** is genuinely well tested.
- **Server's** 92% is mostly collateral. A server fault makes a median of 59
  tests fail at once, and only 15 of its 142 catches involved 5 or fewer
  failing tests. `server_checks`, the suite aimed at the server, caught just
  41 of 155 (26%).
- **Android's** 18% says nothing about the emulator tier. It shows that almost
  all Android logic is tested only there. 339 of its 341 unviable mutants are
  function-body replacements that do not type-check against JNI and UI types.

## Well-tested areas

| Area | Score | Notes |
|---|---|---|
| `packages/core/src/render.rs` | 89% (79/89) | Mostly caught by `parity_snapshot` |
| `packages/core/src/java_port/jfmt.rs` | 90% (56/62) | `jfmt::tests::java_double_strings` + parity |
| `packages/core/src/drawing_actions.rs` | 90% (18/20) | |
| `packages/core/src/java_port/json.rs` | 87% (26/30) | `json::tests::round_trips` |
| `packages/core/src/endpoint.rs` | 88% (7/8) | `tests/https.rs` |
| `shape_catalog.rs`, `ink_geometry.rs`, `layer_editor.rs`, `actions.rs` | 100% | small samples (3–9 each) |
| `apps/server/server.py` | 95% (122/128) | mostly collateral, see above |
| `apps/android/src/network.rs`, `tool_display.rs` | 100% | the few Android modules with focused unit tests |

## Weakly tested areas

### CLI rendering and probes (`notemaker_cli`, 44%)

- **Probe binaries: 0%.** `bin/coverage_parity.rs`, `bin/latency_probe.rs`,
  `bin/cache_footprint_probe.rs`, `bin/stroke_svg_raster.rs` and `probe.rs`
  catch nothing (0/17). These are measurement tools. Nothing checks their
  output, apart from `stroke_svg_raster` being used for rendering.
- **`png.rs` (CLI PNG export): 11% (2/19).** None of the mutants in
  `Png::draw_dots`, GIF/JPEG decoding, `styled_text` or `cached_image` were
  caught. Wrong colours, offsets or skipped dots in exported PNGs go unnoticed.
  Tests check that exports exist and have the right size, not their pixels.
- **`canvas.rs`: 33% (7/21).** `Path::flatten` lost 8 of 12 mutants (curve
  flattening arithmetic), and `Path::add_oval` lost 2 of 3.
- **`ui_hub.rs`: 0% (0/4).** The live UI session hub is exercised by the
  `ui_sessions_android` emulator test, not the local tier.
- **`local_proxy.rs`: 60% (35/58).** The survivors cluster in selection
  leases:
  - `forward_selection_change`: 3 survived, 0 caught. One survivor is
    `local_proxy.rs:2150` (`released = succeeded && …` → `||`). That line is
    the stale-lease race fix recorded in `docs/verification.md` on 2026-11-03.
    No deterministic test pins it. Only the timing-dependent (and flaky)
    `test_cross_proxy_preview_clears_on_deselect_and_commit` goes near it, and
    that test passed on all three mutants.
  - `heartbeat_selection`, including replacing it with `Ok(empty map)`: 2
    survived, 0 caught. Lease heartbeats are not checked.
  - `apply_remote_leases` and `uploads_changes`: 2 survived each.
  - `release`, `release_actor`, `acquire_selection` and `snapshot_state`:
    single survivors.

### Core gaps (`notemaker_core`)

- **`java_port/jmath.rs`: 47% (27/57).** `acos` lost 15 of 17, and `hypot`
  and `atan2` lost 5 each. These fdlibm ports are checked only at a few
  special values (`jmath::tests::fdlibm_specials`). Their branches for
  ordinary arguments have no test.
- **`region_fill.rs::create`: 9 survived, 17 caught.** Flood fill boundary
  arithmetic.
- **`render.rs::interior_overlap`** (6 survived), **`svg_asset.rs::generated_dots`**
  (4) and **`jfmt::Buffer::dtoa`** (4).
- **`timing.rs`: 0/2** and `checkpoint_point_prefixes.rs::Encoder::operation`:
  0/2.
- `Endpoint::cancel -> false` was caught only by a suite timeout. No test
  asserts that cancellation interrupts a request promptly.

### Server

- **`ui_sessions.py`: 60% (9/15).** Replacing `UiSessions.__init__` or
  `command` with `return None` survives. The local tier never drives UI
  sessions; only the emulator tier does.
- `server.py` survivors:
  - `server.py:59`, `server.py:80`: GIF frame and erasure-polygon validation
    (`or` → `and`).
  - `server.py:477`, `server.py:499`: conflict and connector bookkeeping.
  - `server.py:290`: the `Cache-Control: no-store` header can be dropped.

### Android (unit tests only)

These functions catch nothing at the unit level: `viewport_check::run`,
`Integration::main_actions`, `Integration::run_action`,
`interaction_performance::run`, `CanvasView::on_touch_event`,
`CanvasView::on_draw`, `EditorUi::show_popup`, and every `on_draw`. That is
expected for UI and instrumentation code that only the emulator tier runs.
It does mean, though, that a regression in touch handling or drawing is
caught only by a 25-minute emulator pass, or by hardware. `main_activity.rs`
scores 1% (1/101), `editor_ui.rs` 9% (6/68) and `integration.rs` 4% (2/50).

## Which tests detect faults

### Suites

Caught mutants per suite (any failing test, crash or timeout). "Sole" means
no other suite caught that mutant; "per min" is catches per minute of
baseline suite runtime.

| Suite | Core | CLI | Server | Android | Sole | Runtime | Per min |
|---|---|---|---|---|---|---|---|
| `cargo test` (all Rust) | 398 | 0 | – | 98 | 289 | 53 s | 563 |
| `coverage_checks` | 136 | 54 | 113 | – | 12 | 22 s | 815 |
| `drawing_tools_checks` | 122 | 38 | 113 | – | 2 | 26 s | 637 |
| `drawing_browser` | 105 | 36 | 127 | – | 2 | 17 s | 952 |
| `cli_checks` | 93 | 40 | 137 | – | 3 | 65 s | 251 |
| `proxy_checks` | 89 | 33 | 138 | – | 4 | 36 s | 429 |
| `stroke_rendering_checks` | 68 | 27 | 113 | – | 2 | 41 s | 305 |
| `selection_checks` | 67 | 39 | 135 | – | 4 | 42 s | 348 |
| `topology_properties` | 59 | 35 | 137 | – | 3 | 122 s | 114 |
| `conflict_checks` | 55 | 32 | 125 | – | 2 | 71 s | 179 |
| `drawing_checks` | 49 | 31 | 135 | – | 1 | 13 s | 1000 |
| `framework_checks` | 31 | 30 | 48 | – | 0 | 9 s | 719 |
| `chat_checks` | 28 | 30 | 35 | – | 3 | 29 s | 193 |
| `server_checks` | 0 | 0 | 41 | – | 0 | 3 s | 820 |

- **The Rust tests carry the core.** They catch 398 of the 419 caught core
  mutants. The local Python tier adds only 21 core catches that the Rust tests
  miss. For the CLI it is the reverse: the CLI crate has no unit tests, so all
  74 CLI catches come from Python suites.
- **No suite is redundant except two.** A greedy cover needs 12 of the 14
  suites to keep every catch. It saves only `framework_checks` and
  `server_checks`, about 12 s of 548 suite-seconds.
- **Costly suites.** `topology_properties` (122 s, 114 catches per minute) and
  `conflict_checks` (71 s, 179 per minute) are the most expensive per catch,
  but each still catches 2–3 mutants nobody else does.
- **`drawing_browser`** is the most cost-effective suite. Its single test is
  described below.

### Individual tests

"Targeted" counts catches where 5 or fewer tests failed, i.e. the test
pinpointed the fault instead of failing along with everything else.

| Test | Catches | Targeted | Sole |
|---|---|---|---|
| `parity_snapshot::fixtures_match_java_snapshot` (Rust) | 359 | 273 | 136 |
| `DrawingBrowser.test_cli_synced_shapes_render_as_shapes` | 267 | 27 | 0 |
| `ProxyChecks.test_proxy_offline_concurrent_history_reconnect_and_restart` | 215 | 6 | 0 |
| `CoverageChecks.test_masks_follow_object_resize_and_ink_rotation` | 211 | 8 | 0 |
| `CoverageChecks.test_image_alpha_white_pixels_holes_rotation_and_svg` | 210 | 21 | 2 |
| `StrokeRenderingChecks.test_self_crossing_deposits_same_ink_as_two_crossing_lines` | 200 | 18 | 0 |
| `DrawingToolsChecks.test_templates_and_png_export` | 191 | 18 | 1 |
| `jfmt::tests::java_double_strings` (Rust) | 31 | 13 | – |
| `svg_asset::tests::dot_grid_fast_path` (Rust) | 17 | 12 | – |
| Android `network::tests::server_addresses_match_java` / `compose_matches_java` | 27 / 23 | 27 / 23 | – |

- **One test is the safety net for the core.**
  `parity_snapshot::fixtures_match_java_snapshot` catches 367 of the 419
  caught core mutants and is the only catcher of 136. Without it, core's
  score would fall from 78% to about 51%. It compares rendered fixtures with
  the Java reference snapshot, so any change to rendering, number formatting
  or JSON output shows up. It also means a deliberate rendering change
  requires regenerating that snapshot, and one test failure then says little
  about where the fault is.
- **Broad end-to-end tests.** Most Python tests catch 50–270 mutants but are
  rarely the only catcher, because they all fail together when the CLI or
  proxy breaks. The rendering and coverage tests (`drawing_browser`,
  `coverage_checks`, `stroke_rendering_checks`, `test_templates_and_png_export`)
  have the most targeted catches among Python tests.
- **Focused unit tests** (`jfmt`, `json`, `svg_asset`, the Android
  `network`/`tool_display`/`org_json` tests) catch fewer mutants, but almost
  always in a targeted way.

### Tests that caught nothing (15 of 171)

- **All 8 `FrameworkChecks` tests.** They test the test runner and harness
  (`tests/run.py`, upgrade workflow, resource claims), not product code. They
  are expected to catch nothing here.
- **Android unit tests:** `document_browser::tests::sort_order`,
  `field::tests::reentrant_reads`,
  `interaction_performance::tests::not_false_matches_boolean_false_equals`,
  `java_port::monitor::tests::reentrant_and_wait`, and
  `tool_display::tests::unparsable_arguments`.
- **Core unit tests:** `ids::tests::name_uuid` and
  `tls::tests::android_directory_layout_and_removed_list`.

With a 10% sample, a test that catches nothing here can still catch mutants
outside the sample. These are candidates for review, not deletion.

### Duplicate executions

7 `ServerChecks` tests run 9 times per local-tier pass. `drawing_checks`,
`selection_checks`, `cli_checks`, `proxy_checks`, `topology_properties` and
`framework_checks` either subclass `server_checks.ServerChecks` or import the
class into their module namespace, where `unittest` collects it again. That
is 56 redundant test executions per pass. They found nothing that the first
execution missed. Importing the module (`import server_checks`) instead of
the class, or moving the shared server fixture into a base class without
`test_` methods, would remove them.

### Flaky tests

`SelectionChecks.test_cross_proxy_preview_clears_on_deselect_and_commit`
failed in 1 of 10 baselines. It also failed and then passed on rerun after
50 of about 680 mutants unrelated to selection: a failure rate of roughly 7%
under load (`verification.md` measured 1 in 15 on its own before the
2026-11-03 fix). Other failures that did not repeat on rerun:
`ChatChecks.test_reuse_agent_proxy_after_server_restart` (4),
`DrawingBrowser.test_cli_synced_shapes_render_as_shapes` (3),
`CliChecks.test_edit_during_sync_and_lost_ack` (2), and single failures in
`chat_checks`, `topology_properties`, `proxy_checks` and
`framework_checks.NetworkHarnessChecks` fixtures.

## Operational findings

- **Mutants can exhaust host memory.** While the run was in
  `packages/core/src/svg_asset.rs`, mutated `notemaker` processes grew to 9 GB
  and 13.7 GB. That triggered the kernel's global OOM killer, which killed
  unrelated desktop processes and the first two runs. The driver now runs each
  command in a systemd scope with `MemoryMax=4G` and no swap
  (`NOTEMAKER_MUTANT_MEMORY`), so only the runaway process is killed. Such a
  mutant then counts as caught. The real code bounds SVG input (256 KiB,
  10,000 nodes), so this comes from mutated loop logic, not a product defect.
- **Restored files need fresh mtimes.** Restoring a worker copy with
  `rsync -a` kept the original, older mtime, so cargo reused a stale build of
  the mutated file. One baseline showed a false `parity_snapshot` failure.
  The driver now restores files by content with fresh mtimes. Records that
  could have been affected (the first job on each worker after a restart)
  were rerun, and no verdict changed.

## Recommendations

1. **CLI rendering.** Add pixel assertions to PNG export tests: dot templates,
   GIF/JPEG frames, styled text. Add a unit test for `canvas::Path::flatten`
   and `add_oval` against known point lists.
2. **Selection leases.** Add a deterministic test for the
   `forward_selection_change` restored-lease path (inject the stale upstream
   copy instead of racing for it). Also add tests for `heartbeat_selection`
   expiry and `apply_remote_leases`.
3. **Math ports.** Test `jmath::acos`, `hypot` and `atan2` against
   `StrictMath` values over ordinary ranges, not only special values.
   Generating them with the Java reference CLI (as `parity.sh` does) would
   work.
4. **Server.** Add `server_checks` cases for GIF frame bounds, erasure-polygon
   size limits, connector deletion, and a check that UI session commands work
   without an emulator.
5. **Android.** Move pure logic out of `main_activity.rs`, `editor_ui.rs` and
   `integration.rs` (touch-gesture state machines, viewport maths, popup
   layout) into functions with unit tests. The emulator tier is too slow to
   serve as the only check.
6. **Test suite hygiene.** Remove the duplicate `ServerChecks` executions,
   fix or quarantine the flaky cross-proxy preview test, and add a
   cancellation-latency assertion for `Endpoint::cancel`.

## Not verified

- **Emulator and hardware tiers.** No mutant ran against the emulator tier, so
  Android scores reflect unit tests only. No mutant ran against BOOX hardware.
- **Sampling error.** About 10% of mutants were sampled. Per-file scores with
  fewer than about 10 viable mutants have wide error bars.
- **Python mutants are simpler than Rust ones.** They use a hand-written
  operator set, not a full mutation tool, so the server's score is not
  directly comparable with the Rust scores.
- **Lint and parity tooling.** `tools/lints/notemaker_lints` and the Java
  parity harness (`packages/core/tests/parity.sh`) were not mutated.
