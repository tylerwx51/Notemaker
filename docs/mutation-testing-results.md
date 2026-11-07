# Mutation testing results (generated)

Baseline runs: 10; stable test executions: 227 (171 distinct tests); flaky tests excluded: 1; tests failing in every baseline: 0.
Failures that did not repeat on the confirmation rerun (dropped): 69.

## Outcomes

| Verdict | Count |
|---|---|
| killed | 731 |
| survived | 667 |
| unviable | 367 |
| timeout | 2 |

## By package

| Score | Caught / viable | Unviable | Name |
|---|---|---|---|
| 18% | 98/537 | 341 | `notemaker_android` |
| 44% | 74/168 | 12 | `notemaker_cli` |
| 78% | 419/540 | 14 | `notemaker_core` |
| 92% | 142/155 | 0 | `server` |

## By file

| Score | Caught / viable | Unviable | Name |
|---|---|---|---|
| 0% | 0/16 | 2 | `apps/android/src/interaction_performance.rs` |
| 0% | 0/11 | 14 | `apps/android/src/agent_ui.rs` |
| 0% | 0/9 | 8 | `apps/android/src/android_canvas.rs` |
| 0% | 0/8 | 9 | `apps/android/src/java_port/jvm.rs` |
| 0% | 0/8 | 8 | `apps/android/src/jx.rs` |
| 0% | 0/7 | 14 | `apps/android/src/document_browser.rs` |
| 0% | 0/5 | 0 | `apps/cli/src/bin/coverage_parity.rs` |
| 0% | 0/5 | 3 | `apps/android/src/stroke_crossings.rs` |
| 0% | 0/4 | 1 | `apps/cli/src/bin/latency_probe.rs` |
| 0% | 0/4 | 1 | `apps/cli/src/ui_hub.rs` |
| 0% | 0/3 | 0 | `apps/cli/src/bin/stroke_svg_raster.rs` |
| 0% | 0/3 | 3 | `apps/android/src/java_port/concurrent.rs` |
| 0% | 0/3 | 0 | `apps/cli/src/bin/cache_footprint_probe.rs` |
| 0% | 0/2 | 1 | `apps/cli/src/probe.rs` |
| 0% | 0/2 | 1 | `packages/core/src/timing.rs` |
| 0% | 0/2 | 0 | `apps/android/src/lib.rs` |
| 0% | 0/1 | 1 | `apps/android/src/android_consts.rs` |
| 0% | 0/1 | 3 | `apps/android/src/app_replica.rs` |
| 0% | 0/1 | 1 | `apps/android/src/error.rs` |
| 0% | 0/1 | 2 | `apps/android/src/java_port/math.rs` |
| 0% | 0/1 | 2 | `apps/android/src/java_port/monitor.rs` |
| 0% | 0/1 | 3 | `apps/android/src/replica_service.rs` |
| 0% | 0/1 | 4 | `apps/android/src/stroke_check.rs` |
| 0% | 0/1 | 23 | `apps/android/src/ui.rs` |
| 1% | 1/101 | 55 | `apps/android/src/main_activity.rs` |
| 4% | 2/50 | 28 | `apps/android/src/integration.rs` |
| 8% | 1/12 | 0 | `apps/android/src/java_port/rect_f.rs` |
| 9% | 6/68 | 67 | `apps/android/src/editor_ui.rs` |
| 11% | 2/19 | 2 | `apps/cli/src/png.rs` |
| 13% | 4/31 | 12 | `apps/android/src/animation_player.rs` |
| 15% | 5/33 | 9 | `apps/android/src/viewport_check.rs` |
| 21% | 7/34 | 11 | `apps/android/src/tile_ink_cache.rs` |
| 25% | 1/4 | 0 | `apps/android/src/java_port/exception.rs` |
| 33% | 7/21 | 1 | `apps/cli/src/canvas.rs` |
| 33% | 2/6 | 0 | `apps/android/src/erase_performance.rs` |
| 33% | 1/3 | 0 | `packages/core/src/java_port/file.rs` |
| 36% | 8/22 | 8 | `apps/android/src/performance.rs` |
| 40% | 2/5 | 1 | `apps/cli/src/raster.rs` |
| 40% | 2/5 | 1 | `apps/cli/src/typeface.rs` |
| 42% | 5/12 | 5 | `apps/android/src/coverage_check.rs` |
| 43% | 3/7 | 1 | `apps/cli/src/item_coverage.rs` |
| 43% | 3/7 | 1 | `apps/android/src/ink_cache.rs` |
| 45% | 14/31 | 23 | `apps/android/src/java_port/org_json.rs` |
| 47% | 27/57 | 0 | `packages/core/src/java_port/jmath.rs` |
| 50% | 2/4 | 0 | `packages/core/src/checkpoint_point_prefixes.rs` |
| 50% | 2/4 | 0 | `packages/core/src/net.rs` |
| 50% | 1/2 | 1 | `apps/cli/src/java_port/jutil.rs` |
| 50% | 1/2 | 1 | `packages/core/src/ids.rs` |
| 50% | 1/2 | 1 | `packages/core/src/style.rs` |
| 50% | 1/2 | 1 | `packages/core/src/tls.rs` |
| 50% | 1/2 | 2 | `apps/android/src/checkpoint_performance.rs` |
| 50% | 1/2 | 6 | `apps/android/src/ui_actions.rs` |
| 60% | 9/15 | 0 | `apps/server/ui_sessions.py` |
| 60% | 35/58 | 1 | `apps/cli/src/local_proxy.rs` |
| 67% | 18/27 | 0 | `packages/core/src/region_fill.rs` |
| 67% | 2/3 | 0 | `packages/core/src/wire.rs` |
| 69% | 18/26 | 1 | `apps/cli/src/cli_main.rs` |
| 71% | 10/14 | 0 | `packages/core/src/geometry.rs` |
| 71% | 5/7 | 1 | `packages/core/src/coverage.rs` |
| 73% | 16/22 | 2 | `packages/core/src/journal.rs` |
| 73% | 11/15 | 0 | `packages/core/src/image_asset.rs` |
| 73% | 11/15 | 3 | `packages/core/src/store.rs` |
| 75% | 9/12 | 0 | `packages/core/src/gif_asset.rs` |
| 77% | 10/13 | 0 | `packages/core/src/model.rs` |
| 77% | 27/35 | 9 | `apps/android/src/java_port/uri.rs` |
| 77% | 44/57 | 0 | `packages/core/src/svg_asset.rs` |
| 79% | 15/19 | 0 | `packages/core/src/collaboration.rs` |
| 80% | 16/20 | 1 | `packages/core/src/draw_item.rs` |
| 87% | 26/30 | 0 | `packages/core/src/java_port/json.rs` |
| 88% | 7/8 | 0 | `packages/core/src/endpoint.rs` |
| 89% | 79/89 | 0 | `packages/core/src/render.rs` |
| 90% | 18/20 | 0 | `packages/core/src/drawing_actions.rs` |
| 90% | 56/62 | 1 | `packages/core/src/java_port/jfmt.rs` |
| 92% | 11/12 | 0 | `apps/server/svg_asset.py` |
| 95% | 122/128 | 0 | `apps/server/server.py` |
| 100% | 9/9 | 0 | `packages/core/src/shape_catalog.rs` |
| 100% | 6/6 | 0 | `packages/core/src/ink_geometry.rs` |
| 100% | 5/5 | 1 | `apps/android/src/network.rs` |
| 100% | 4/4 | 0 | `packages/core/src/layer_editor.rs` |
| 100% | 3/3 | 0 | `packages/core/src/actions.rs` |
| 100% | 3/3 | 0 | `packages/core/src/java_port/exception.rs` |
| 100% | 3/3 | 0 | `packages/core/src/java_port/url.rs` |
| 100% | 3/3 | 0 | `apps/android/src/tool_display.rs` |
| 100% | 3/3 | 0 | `apps/cli/src/area.rs` |
| 100% | 2/2 | 0 | `packages/core/src/clock.rs` |
| 100% | 2/2 | 0 | `apps/android/src/java_port/integer.rs` |
| 100% | 1/1 | 0 | `apps/cli/src/bin/notemaker.rs` |
| 100% | 1/1 | 2 | `packages/core/src/error.rs` |

## By mutation genre

| Score | Caught / viable | Unviable | Name |
|---|---|---|---|
| 0% | 0/2 | 0 | `notemaker_cli:MatchArmGuard` |
| 0% | 0/2 | 0 | `notemaker_cli:StructField` |
| 13% | 3/23 | 0 | `notemaker_android:MatchArm` |
| 15% | 61/395 | 2 | `notemaker_android:BinaryOperator` |
| 18% | 6/33 | 0 | `notemaker_android:UnaryOperator` |
| 29% | 2/7 | 0 | `notemaker_android:MatchArmGuard` |
| 33% | 26/79 | 339 | `notemaker_android:FnValue` |
| 38% | 44/116 | 0 | `notemaker_cli:BinaryOperator` |
| 43% | 3/7 | 0 | `notemaker_cli:UnaryOperator` |
| 50% | 1/2 | 0 | `notemaker_core:MatchArmGuard` |
| 62% | 21/34 | 12 | `notemaker_cli:FnValue` |
| 64% | 16/25 | 0 | `notemaker_core:UnaryOperator` |
| 76% | 313/410 | 0 | `notemaker_core:BinaryOperator` |
| 78% | 7/9 | 0 | `server:FnValue` |
| 85% | 77/91 | 14 | `notemaker_core:FnValue` |
| 86% | 18/21 | 0 | `server:BoolOperator` |
| 86% | 6/7 | 0 | `notemaker_cli:MatchArm` |
| 89% | 24/27 | 0 | `server:DeleteCall` |
| 94% | 64/68 | 0 | `server:CompareNegation` |
| 96% | 23/24 | 0 | `server:UnaryNot` |
| 100% | 12/12 | 0 | `notemaker_core:MatchArm` |
| 100% | 6/6 | 0 | `server:BoolConstant` |

## Suites

Mutants each suite caught, per package (viable mutants in parentheses). "Sole" counts mutants no other suite caught; "per min" is caught mutants per minute of baseline suite runtime.

| Suite | notemaker_android (537) | notemaker_cli (168) | notemaker_core (540) | server (155) | Sole | Runtime s | Per min |
|---|---|---|---|---|---|---|---|
| `rust` | 98 | 0 | 398 | 0 | 289 | 52.9 | 562.6 |
| `coverage_checks` | 0 | 54 | 136 | 113 | 12 | 22.3 | 815.2 |
| `drawing_tools_checks` | 0 | 38 | 122 | 113 | 2 | 25.7 | 637.4 |
| `cli_checks` | 0 | 40 | 93 | 137 | 3 | 64.6 | 250.8 |
| `drawing_browser` | 0 | 36 | 105 | 127 | 2 | 16.9 | 951.5 |
| `proxy_checks` | 0 | 33 | 89 | 138 | 4 | 36.4 | 428.6 |
| `selection_checks` | 0 | 39 | 67 | 135 | 4 | 41.5 | 348.4 |
| `topology_properties` | 0 | 35 | 59 | 137 | 3 | 121.5 | 114.1 |
| `drawing_checks` | 0 | 31 | 49 | 135 | 1 | 12.9 | 1000.0 |
| `conflict_checks` | 0 | 32 | 55 | 125 | 2 | 71.0 | 179.2 |
| `stroke_rendering_checks` | 0 | 27 | 68 | 113 | 2 | 40.9 | 305.1 |
| `framework_checks` | 0 | 30 | 31 | 48 | 0 | 9.1 | 718.7 |
| `chat_checks` | 0 | 30 | 28 | 35 | 3 | 28.9 | 193.1 |
| `server_checks` | 0 | 0 | 0 | 41 | 0 | 3.0 | 820.0 |

## Tests by kills

Duplicated executions (a `ServerChecks` test inherited by another suite) are collapsed. "Sole" = the only test that caught the mutant. "Targeted" = kills where at most 5 tests failed, i.e. not a fault that breaks everything.

| Test | Kills | Targeted | Sole kills | Executions per run |
|---|---|---|---|---|
| `rust:parity_snapshot:tests/parity_snapshot.rs::fixtures_match_java_snapshot` | 359 | 273 | 142 | 1 |
| `py:drawing_browser.DrawingBrowser.test_cli_synced_shapes_render_as_shapes` | 267 | 27 | 2 | 1 |
| `py:proxy_checks.ProxyChecks.test_proxy_offline_concurrent_history_reconnect_and_restart` | 215 | 6 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_masks_follow_object_resize_and_ink_rotation` | 211 | 8 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_image_alpha_white_pixels_holes_rotation_and_svg` | 210 | 21 | 2 | 1 |
| `py:cli_checks.CliChecks.test_svg_placement_history_export_and_sync` | 208 | 6 | 0 | 1 |
| `py:cli_checks.CliChecks.test_matplotlib_chart_gallery_offline_render_and_sync` | 205 | 7 | 0 | 1 |
| `py:cli_checks.CliChecks.test_matplotlib_grid_is_one_atomic_note_diagram` | 203 | 6 | 0 | 1 |
| `py:stroke_rendering_checks.StrokeRenderingChecks.test_self_crossing_deposits_same_ink_as_two_crossing_lines` | 200 | 18 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_empty_coverage_and_flood_fill_respect_erased_boundaries` | 199 | 5 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_offline_masks_retain_both_conflicting_versions_on_reconnect` | 199 | 0 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_partial_erase_keeps_identity_other_layer_and_atomic_undo` | 199 | 1 | 0 | 1 |
| `py:drawing_checks.DrawingChecks.test_shape_catalog_crud_history_render_and_proxy_sync` | 198 | 6 | 1 | 1 |
| `py:conflict_checks.ConflictChecks.test_same_actor_divergence_preserves_both_versions_and_resolution_history` | 196 | 2 | 1 | 1 |
| `py:coverage_checks.CoverageChecks.test_repeated_collinear_and_backtracking_erase_points` | 196 | 6 | 1 | 1 |
| `py:selection_checks.SelectionChecks.test_transform_commit_uploads_without_an_explicit_sync_command` | 195 | 4 | 1 | 1 |
| `py:topology_properties.TopologyProperties.test_cycles_sole_writer_and_offline_chain` | 192 | 4 | 1 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_templates_and_png_export` | 191 | 18 | 1 | 1 |
| `py:stroke_rendering_checks.StrokeRenderingChecks.test_translucent_curves_have_no_seams_between_ribbon_passes` | 191 | 12 | 0 | 1 |
| `py:cli_checks.CliChecks.test_document_deletion_offline_restart_and_stale_replica` | 190 | 0 | 0 | 1 |
| `py:conflict_checks.ConflictChecks.test_page_two_branch_multi_edit_survives_proxy_restart` | 189 | 0 | 0 | 1 |
| `py:cli_checks.CliChecks.test_svg_intrinsic_size_and_isolated_export` | 189 | 7 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_hollow_shape_pressure_ink_and_topmost_layer_candidates` | 187 | 7 | 2 | 1 |
| `py:coverage_checks.CoverageChecks.test_appended_ink_does_not_reuse_old_action_metadata` | 187 | 0 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_fill_region_closed_by_multiple_strokes_and_reject_open_space` | 187 | 10 | 0 | 1 |
| `py:conflict_checks.ConflictChecks.test_hide_vs_edit_conflict_preserves_activity_snapshots` | 187 | 3 | 1 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_partial_eraser_masks_and_undo_restores_as_one_action` | 185 | 0 | 0 | 1 |
| `py:cli_checks.CliChecks.test_edit_during_sync_and_lost_ack` | 185 | 1 | 0 | 1 |
| `py:topology_properties.TopologyProperties.test_generated_trees_converge` | 183 | 5 | 0 | 1 |
| `py:conflict_checks.ConflictChecks.test_different_actor_divergence_and_sequential_edit` | 182 | 1 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_clipboard_cut_paste_preserves_atomic_diagram_and_references` | 180 | 2 | 0 | 1 |
| `py:conflict_checks.ConflictChecks.test_simultaneous_resolutions_create_a_new_reviewable_conflict` | 180 | 1 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_erase_transparent_interior_is_noop_and_invalid_masks_are_atomic` | 179 | 6 | 1 | 1 |
| `py:conflict_checks.ConflictChecks.test_same_actor_sequential_edits_do_not_conflict` | 179 | 0 | 0 | 1 |
| `py:selection_checks.SelectionChecks.test_closing_observer_does_not_release_remote_selection_owner` | 178 | 1 | 1 | 1 |
| `py:selection_checks.SelectionChecks.test_cli_copy_cut_paste_uses_local_clipboard` | 178 | 2 | 1 | 1 |
| `py:cli_checks.CliChecks.test_reconnect_conflict_convergence_and_retry` | 178 | 0 | 0 | 1 |
| `py:cli_checks.CliChecks.test_server_identity_and_connector_validation` | 177 | 0 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_stroke_and_page_erase_cancel_on_invalid_input` | 177 | 2 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_native_and_closed_stroke_fill` | 174 | 1 | 0 | 1 |
| `py:selection_checks.SelectionChecks.test_sibling_selection_race_and_disconnect_propagates` | 173 | 0 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_text_spaces_and_transparent_paint_are_not_coverage` | 173 | 3 | 1 | 1 |
| `py:selection_checks.SelectionChecks.test_offline_selection_can_commit_atomically` | 172 | 3 | 0 | 1 |
| `py:selection_checks.SelectionChecks.test_page_one_layer_lock_blocks_hide_and_undo` | 167 | 0 | 0 | 1 |
| `py:selection_checks.SelectionChecks.test_transform_rotation_then_flip_composes_about_pivot` | 166 | 4 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_material_parameters_validate_and_change_rendered_strokes` | 164 | 2 | 0 | 1 |
| `py:proxy_checks.ProxyChecks.test_checkpoint_reconstructs_stroke_point_prefixes` | 163 | 3 | 1 | 1 |
| `py:cli_checks.CliChecks.test_offline_diagram_edit_history_and_render` | 162 | 1 | 0 | 1 |
| `py:selection_checks.SelectionChecks.test_locked_sources_block_fill_duplicate_and_page_move` | 161 | 1 | 1 | 1 |
| `py:cli_checks.CliChecks.test_reject_partial_invalid_action` | 161 | 1 | 0 | 1 |
| `py:selection_checks.SelectionChecks.test_preview_is_ephemeral_cancellable_and_expires_with_lease` | 160 | 0 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_excessively_complex_fill_is_rejected_without_an_action` | 160 | 3 | 0 | 1 |
| `py:coverage_checks.CoverageChecks.test_wrong_layer_batch_rejected_before_leases_and_session_cannot_change_scope` | 159 | 1 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_new_style_validation_and_preservation` | 158 | 1 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_append_and_editor_settings_commit_atomically` | 157 | 1 | 1 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_layer_duplicate_merge_and_order_are_atomic` | 157 | 1 | 0 | 1 |
| `py:selection_checks.SelectionChecks.test_selection_reference_validation_rejects_wrong_scope_and_inactive_items` | 155 | 1 | 0 | 1 |
| `py:proxy_checks.ProxyChecks.test_strict_number_parser` | 154 | 1 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_material_paths_do_not_accumulate_with_dense_input_samples` | 154 | 3 | 0 | 1 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_object_put_returns_durable_post_edit_snapshot` | 153 | 0 | 0 | 1 |

## Tests by targeted kills

| Test | Targeted | Kills |
|---|---|---|
| `rust:parity_snapshot:tests/parity_snapshot.rs::fixtures_match_java_snapshot` | 273 | 359 |
| `py:drawing_browser.DrawingBrowser.test_cli_synced_shapes_render_as_shapes` | 27 | 267 |
| `rust:notemaker_android:src/lib.rs::network::tests::server_addresses_match_java` | 27 | 27 |
| `rust:notemaker_android:src/lib.rs::network::tests::compose_matches_java` | 23 | 23 |
| `py:coverage_checks.CoverageChecks.test_image_alpha_white_pixels_holes_rotation_and_svg` | 21 | 210 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_templates_and_png_export` | 18 | 191 |
| `py:stroke_rendering_checks.StrokeRenderingChecks.test_self_crossing_deposits_same_ink_as_two_crossing_lines` | 18 | 200 |
| `rust:notemaker_core:src/lib.rs::java_port::jfmt::tests::java_double_strings` | 13 | 31 |
| `py:stroke_rendering_checks.StrokeRenderingChecks.test_translucent_curves_have_no_seams_between_ribbon_passes` | 12 | 191 |
| `rust:notemaker_android:src/lib.rs::java_port::org_json::tests::formatting` | 12 | 24 |
| `rust:notemaker_core:src/lib.rs::svg_asset::tests::dot_grid_fast_path` | 12 | 17 |
| `py:drawing_tools_checks.DrawingToolsChecks.test_fill_region_closed_by_multiple_strokes_and_reject_open_space` | 10 | 187 |
| `rust:notemaker_core:src/lib.rs::java_port::json::tests::round_trips` | 9 | 34 |
| `py:coverage_checks.CoverageChecks.test_masks_follow_object_resize_and_ink_rotation` | 8 | 211 |
| `py:server_checks.ServerChecks.test_embedded_gif_is_validated_before_commit` | 8 | 33 |
| `rust:notemaker_android:src/lib.rs::tool_display::tests::nested_and_long` | 8 | 8 |
| `py:coverage_checks.CoverageChecks.test_hollow_shape_pressure_ink_and_topmost_layer_candidates` | 7 | 187 |
| `py:cli_checks.CliChecks.test_matplotlib_chart_gallery_offline_render_and_sync` | 7 | 205 |
| `py:cli_checks.CliChecks.test_svg_intrinsic_size_and_isolated_export` | 7 | 189 |
| `rust:notemaker_android:src/lib.rs::coverage_check::tests::compare_counts_premultiplied_red` | 7 | 7 |
| `py:coverage_checks.CoverageChecks.test_erase_transparent_interior_is_noop_and_invalid_masks_are_atomic` | 6 | 179 |
| `py:proxy_checks.ProxyChecks.test_proxy_offline_concurrent_history_reconnect_and_restart` | 6 | 215 |
| `py:cli_checks.CliChecks.test_svg_placement_history_export_and_sync` | 6 | 208 |
| `py:drawing_checks.DrawingChecks.test_shape_catalog_crud_history_render_and_proxy_sync` | 6 | 198 |
| `py:cli_checks.CliChecks.test_matplotlib_grid_is_one_atomic_note_diagram` | 6 | 203 |
| `py:coverage_checks.CoverageChecks.test_repeated_collinear_and_backtracking_erase_points` | 6 | 196 |
| `rust:append_scope:tests/append_scope.rs::rejected_action_edits_keep_the_cache` | 6 | 39 |
| `rust:notemaker_android:src/lib.rs::tool_display::tests::read_and_edit` | 6 | 6 |
| `rust:notemaker_android:src/lib.rs::tool_display::tests::grep_and_bash` | 6 | 6 |
| `py:coverage_checks.CoverageChecks.test_empty_coverage_and_flood_fill_respect_erased_boundaries` | 5 | 199 |

## Tests that caught no mutant (15 of 171)

- `py:framework_checks.FrameworkChecks.test_default_is_independent_of_devices`
- `py:framework_checks.FrameworkChecks.test_external_resources_are_explicit`
- `py:framework_checks.FrameworkChecks.test_failed_subprocess_is_recordable`
- `py:framework_checks.FrameworkChecks.test_preservation_accepts_commits_but_rejects_loss_or_changes`
- `py:framework_checks.FrameworkChecks.test_scheduler_uses_recent_suite_history_and_preserves_resource_claims`
- `py:framework_checks.FrameworkChecks.test_skip_can_be_required`
- `py:framework_checks.FrameworkChecks.test_upgrade_workflow_and_preservation_failures_without_hardware`
- `py:framework_checks.FrameworkChecks.test_wrong_network_root_is_rejected_before_any_edits`
- `rust:notemaker_android:src/lib.rs::document_browser::tests::sort_order`
- `rust:notemaker_android:src/lib.rs::field::tests::reentrant_reads`
- `rust:notemaker_android:src/lib.rs::interaction_performance::tests::not_false_matches_boolean_false_equals`
- `rust:notemaker_android:src/lib.rs::java_port::monitor::tests::reentrant_and_wait`
- `rust:notemaker_android:src/lib.rs::tool_display::tests::unparsable_arguments`
- `rust:notemaker_core:src/lib.rs::ids::tests::name_uuid`
- `rust:notemaker_core:src/lib.rs::tls::tests::android_directory_layout_and_removed_list`

## Duplicate executions (56 redundant per run)

- `py:server_checks.ServerChecks.test_reject_invalid_pressure_and_whole_batch`: 9x
- `py:server_checks.ServerChecks.test_polygon_bounds_must_be_positive`: 9x
- `py:server_checks.ServerChecks.test_isolation_cursor_and_idempotency`: 9x
- `py:server_checks.ServerChecks.test_embedded_gif_is_validated_before_commit`: 9x
- `py:server_checks.ServerChecks.test_editable_object_shapes_and_signed_line_bounds`: 9x
- `py:server_checks.ServerChecks.test_deletion_index_is_atomic_and_retains_log`: 9x
- `py:server_checks.ServerChecks.test_collision_rolls_back_entire_batch`: 9x

## Functions with the most surviving mutants

| Survived | Caught | Function |
|---|---|---|
| 18 | 0 | `apps/android/src/viewport_check.rs::run` |
| 16 | 0 | `apps/android/src/integration.rs::Integration::main_actions` |
| 15 | 0 | `apps/android/src/integration.rs::Integration::run_action` |
| 15 | 0 | `apps/android/src/interaction_performance.rs::run` |
| 15 | 2 | `packages/core/src/java_port/jmath.rs::acos` |
| 14 | 0 | `apps/android/src/main_activity.rs::CanvasView::on_touch_event` |
| 9 | 0 | `apps/android/src/main_activity.rs::CanvasView::on_draw` |
| 9 | 17 | `packages/core/src/region_fill.rs::create` |
| 8 | 0 | `apps/android/src/editor_ui.rs::Icon::on_draw` |
| 8 | 0 | `apps/android/src/performance.rs::run` |
| 8 | 4 | `apps/cli/src/canvas.rs::Path::flatten` |
| 7 | 0 | `apps/android/src/android_canvas.rs::<impl Canvas for AndroidCanvas>::draw_svg` |
| 7 | 0 | `apps/android/src/editor_ui.rs::EditorUi::show_popup` |
| 6 | 0 | `apps/android/src/animation_player.rs::AnimationPlayer::controls` |
| 6 | 0 | `apps/android/src/animation_player.rs::AnimationPlayer::draw_controls` |
| 6 | 0 | `apps/android/src/editor_ui.rs::LayerPreview::on_draw` |
| 6 | 0 | `apps/android/src/viewport_check.rs::seam_compare` |
| 6 | 3 | `packages/core/src/render.rs::interior_overlap` |
| 5 | 0 | `apps/cli/src/png.rs::Png<'a>::draw_dots` |
| 5 | 0 | `apps/android/src/animation_player.rs::AnimationPlayer::invalidate` |
| 5 | 0 | `apps/android/src/editor_ui.rs::EditorUi::open_tool` |
| 5 | 0 | `apps/android/src/editor_ui.rs::StrokeSample::on_draw` |
| 5 | 0 | `apps/android/src/integration.rs::Integration::report_state` |
| 5 | 0 | `apps/android/src/main_activity.rs::CanvasView::scribble` |
| 5 | 3 | `packages/core/src/java_port/jmath.rs::hypot` |
| 5 | 5 | `packages/core/src/java_port/jmath.rs::atan2` |
| 5 | 9 | `apps/cli/src/cli_main.rs::run_command` |
| 4 | 0 | `apps/cli/src/bin/latency_probe.rs::run` |
| 4 | 0 | `apps/android/src/coverage_check.rs::cache_pixels_sized` |
| 4 | 0 | `apps/android/src/java_port/rect_f.rs::<impl RectFExt for RectF>::intersect` |
| 4 | 0 | `apps/android/src/main_activity.rs::MainActivity::persist` |
| 4 | 0 | `apps/android/src/main_activity.rs::CanvasView::draw_infinite_template` |
| 4 | 0 | `apps/android/src/main_activity.rs::CanvasView::inverse_preview_viewport` |
| 4 | 0 | `apps/android/src/tile_ink_cache.rs::TileInkCache::draw_monolithic` |
| 4 | 0 | `apps/android/src/tile_ink_cache.rs::TileInkCache::draw_item` |
| 4 | 6 | `packages/core/src/svg_asset.rs::generated_dots` |
| 4 | 15 | `packages/core/src/java_port/jfmt.rs::Buffer::dtoa` |
| 3 | 0 | `apps/cli/src/bin/stroke_svg_raster.rs::run` |
| 3 | 0 | `apps/cli/src/local_proxy.rs::LocalProxy::forward_selection_change` |
| 3 | 0 | `apps/android/src/editor_ui.rs::EditorUi::refresh` |

## Surviving mutants


### `apps/android/src/agent_ui.rs`

- `agent_ui.rs:188:9` replace AgentUi::cancel_stream with ()
- `agent_ui.rs:292:23` replace >= with < in AgentUi::show
- `agent_ui.rs:305:107` replace + with - in AgentUi::show
- `agent_ui.rs:350:17` delete match arm "right" in AgentUi::resize_popup
- `agent_ui.rs:356:97` replace - with + in AgentUi::resize_popup
- `agent_ui.rs:387:9` replace AgentUi::request with ()
- `agent_ui.rs:480:101` replace + with * in AgentUi::hub
- `agent_ui.rs:613:107` delete - in AgentUi::conversation
- `agent_ui.rs:746:26` replace != with == in AgentUi::read_stream
- `agent_ui.rs:812:133` replace - with / in AgentUi::flush
- `agent_ui.rs:835:73` replace || with && in AgentUi::flush

### `apps/android/src/android_canvas.rs`

- `android_canvas.rs:218:9` replace <impl Canvas for AndroidCanvas>::scale with ()
- `android_canvas.rs:251:9` replace <impl Canvas for AndroidCanvas>::draw_bitmap with ()
- `android_canvas.rs:270:55` replace && with || in <impl Canvas for AndroidCanvas>::draw_svg
- `android_canvas.rs:270:88` replace > with < in <impl Canvas for AndroidCanvas>::draw_svg
- `android_canvas.rs:277:51` replace * with / in <impl Canvas for AndroidCanvas>::draw_svg
- `android_canvas.rs:282:36` replace <= with > in <impl Canvas for AndroidCanvas>::draw_svg
- `android_canvas.rs:285:50` replace > with == in <impl Canvas for AndroidCanvas>::draw_svg
- `android_canvas.rs:294:26` replace / with % in <impl Canvas for AndroidCanvas>::draw_svg
- `android_canvas.rs:300:73` replace / with % in <impl Canvas for AndroidCanvas>::draw_svg

### `apps/android/src/android_consts.rs`

- `android_consts.rs:115:50` delete -

### `apps/android/src/animation_player.rs`

- `animation_player.rs:175:47` replace + with - in AnimationPlayer::tick
- `animation_player.rs:201:38` replace && with || in AnimationPlayer::available
- `animation_player.rs:240:68` replace + with - in AnimationPlayer::invalidate
- `animation_player.rs:244:28` replace - with / in AnimationPlayer::invalidate
- `animation_player.rs:246:50` replace + with - in AnimationPlayer::invalidate
- `animation_player.rs:252:22` replace + with - in AnimationPlayer::invalidate
- `animation_player.rs:254:35` replace * with + in AnimationPlayer::invalidate
- `animation_player.rs:273:41` replace && with || in AnimationPlayer::prepare_visible
- `animation_player.rs:305:45` replace + with - in AnimationPlayer::play
- `animation_player.rs:371:28` replace / with % in AnimationPlayer::draw
- `animation_player.rs:407:35` replace * with + in AnimationPlayer::controls
- `animation_player.rs:408:74` replace * with + in AnimationPlayer::controls
- `animation_player.rs:411:25` replace + with * in AnimationPlayer::controls
- `animation_player.rs:411:52` replace + with - in AnimationPlayer::controls
- `animation_player.rs:415:35` replace + with - in AnimationPlayer::controls
- `animation_player.rs:415:75` replace - with / in AnimationPlayer::controls
- `animation_player.rs:459:47` replace - with + in AnimationPlayer::seek_at
- `animation_player.rs:460:66` replace - with + in AnimationPlayer::seek_at
- `animation_player.rs:471:32` replace == with != in AnimationPlayer::touch
- `animation_player.rs:507:58` replace == with != in AnimationPlayer::touch
- `animation_player.rs:548:45` delete ! in AnimationPlayer::draw_controls
- `animation_player.rs:569:102` replace * with / in AnimationPlayer::draw_controls
- `animation_player.rs:570:59` replace * with / in AnimationPlayer::draw_controls
- `animation_player.rs:571:100` replace * with / in AnimationPlayer::draw_controls
- `animation_player.rs:580:41` replace + with * in AnimationPlayer::draw_controls
- `animation_player.rs:584:35` replace + with * in AnimationPlayer::draw_controls
- `animation_player.rs:647:23` replace == with != in AnimationPlayer::show_controls

### `apps/android/src/app_replica.rs`

- `app_replica.rs:170:66` replace && with || in AppReplica::command_with

### `apps/android/src/checkpoint_performance.rs`

- `checkpoint_performance.rs:54:57` replace - with + in measure

### `apps/android/src/coverage_check.rs`

- `coverage_check.rs:78:39` replace == with != in run
- `coverage_check.rs:171:17` delete ! in object_preview
- `coverage_check.rs:208:33` replace && with || in preview_rollback
- `coverage_check.rs:276:66` replace * with + in cache_pixels_sized
- `coverage_check.rs:307:60` replace && with || in cache_pixels_sized
- `coverage_check.rs:345:21` replace == with != in cache_pixels_sized
- `coverage_check.rs:410:36` replace * with / in cache_pixels_sized

### `apps/android/src/document_browser.rs`

- `document_browser.rs:141:5` replace trim -> &str with "xyzzy"
- `document_browser.rs:160:36` replace match guard (x as u32) <= 0xffff with true in lower_unit
- `document_browser.rs:414:17` replace && with || in DocumentBrowser::current
- `document_browser.rs:497:47` replace != with == in DocumentBrowser::preview
- `document_browser.rs:532:96` delete ! in DocumentBrowser::thumbnail
- `document_browser.rs:559:24` replace += with -= in DocumentBrowser::thumbnail
- `document_browser.rs:638:32` delete ! in DocumentBrowser::delete

### `apps/android/src/editor_ui.rs`

- `editor_ui.rs:193:9` delete match arm "oily" in material_range
- `editor_ui.rs:532:5` replace is_instance -> bool with false
- `editor_ui.rs:737:117` delete - in EditorUi::build
- `editor_ui.rs:922:22` replace == with != in EditorUi::refresh
- `editor_ui.rs:957:39` delete ! in EditorUi::refresh
- `editor_ui.rs:988:74` replace - with + in EditorUi::refresh
- `editor_ui.rs:1009:97` delete - in EditorUi::layers_panel
- `editor_ui.rs:1098:81` replace - with + in EditorUi::rebuild_layers
- `editor_ui.rs:1229:57` replace == with != in EditorUi::layer_options
- `editor_ui.rs:1236:142` replace + with - in EditorUi::layer_options
- `editor_ui.rs:1312:13` delete match arm "interact" in EditorUi::open_tool
- `editor_ui.rs:1393:59` replace / with % in EditorUi::open_tool
- `editor_ui.rs:1454:28` delete ! in EditorUi::open_tool
- `editor_ui.rs:1485:13` delete match arm "page" in EditorUi::open_tool
- `editor_ui.rs:1499:63` replace < with <= in EditorUi::open_tool
- `editor_ui.rs:1550:113` replace < with <= in EditorUi::shape_panel
- `editor_ui.rs:1575:35` replace < with == in EditorUi::shape_panel
- `editor_ui.rs:1659:18` replace % with / in EditorUi::color_choices
- `editor_ui.rs:1732:9` replace EditorUi::material_description -> String with String::new()
- `editor_ui.rs:1778:63` replace == with != in EditorUi::pen_panel
- `editor_ui.rs:1882:59` replace | with ^ in EditorUi::network_settings_panel
- `editor_ui.rs:1957:51` replace == with != in EditorUi::fill_panel
- `editor_ui.rs:1995:113` replace || with && in EditorUi::fill_panel
- `editor_ui.rs:2058:47` replace + with * in EditorUi::eraser_panel
- `editor_ui.rs:2467:13` delete match arm "history" in EditorUi::open_panel
- `editor_ui.rs:2558:17` delete match arm "right" in EditorUi::show_popup
- `editor_ui.rs:2568:71` replace - with + in EditorUi::show_popup
- `editor_ui.rs:2570:74` replace - with + in EditorUi::show_popup
- `editor_ui.rs:2578:24` replace < with <= in EditorUi::show_popup
- `editor_ui.rs:2583:83` replace + with - in EditorUi::show_popup
- `editor_ui.rs:2611:48` replace + with * in EditorUi::show_popup
- `editor_ui.rs:2632:17` delete match arm (None, None) in EditorUi::show_popup
- `editor_ui.rs:2733:52` replace - with + in Tool::dispatch_draw
- `editor_ui.rs:2741:39` replace / with % in Tool::dispatch_draw
- `editor_ui.rs:2744:68` replace / with % in Tool::dispatch_draw
- `editor_ui.rs:2754:32` replace == with != in Tool::active
- `editor_ui.rs:2811:61` replace / with % in LayerPreview::on_draw
- `editor_ui.rs:2828:17` replace && with || in LayerPreview::on_draw
- `editor_ui.rs:2844:56` replace - with / in LayerPreview::on_draw
- `editor_ui.rs:2856:44` replace - with / in LayerPreview::on_draw
- `editor_ui.rs:2868:21` replace && with || in LayerPreview::on_draw
- `editor_ui.rs:2882:51` replace - with / in LayerPreview::on_draw
- `editor_ui.rs:2934:56` replace * with + in StrokeSample::on_draw
- `editor_ui.rs:2939:32` replace + with * in StrokeSample::on_draw
- `editor_ui.rs:2939:82` replace + with * in StrokeSample::on_draw
- `editor_ui.rs:2940:40` replace * with + in StrokeSample::on_draw
- `editor_ui.rs:2940:69` replace - with + in StrokeSample::on_draw
- `editor_ui.rs:3022:40` replace - with + in ConflictPreview::on_draw
- `editor_ui.rs:3025:51` replace * with + in ConflictPreview::on_draw
- `editor_ui.rs:3025:103` replace * with + in ConflictPreview::on_draw
- `editor_ui.rs:3100:67` replace / with % in Icon::on_draw
- `editor_ui.rs:3133:13` delete match arm "check" in Icon::on_draw
- `editor_ui.rs:3156:13` delete match arm "pencil" in Icon::on_draw
- `editor_ui.rs:3215:13` delete match arm "interact" in Icon::on_draw
- `editor_ui.rs:3242:13` delete match arm "triangle45" in Icon::on_draw
- `editor_ui.rs:3258:13` delete match arm "cube" in Icon::on_draw
- `editor_ui.rs:3306:13` delete match arm "polyline" in Icon::on_draw
- `editor_ui.rs:3340:23` replace += with -= in Icon::on_draw
- `editor_ui.rs:3368:17` replace < with <= in Icon::path
- `editor_ui.rs:3381:25` replace / with * in Icon::poly
- `editor_ui.rs:3382:29` replace + with - in Icon::poly
- `editor_ui.rs:3473:36` replace == with != in ColorSwatch::on_draw

### `apps/android/src/erase_performance.rs`

- `erase_performance.rs:28:18` replace *= with /= in concurrent_hash_map_order
- `erase_performance.rs:67:12` delete ! in run
- `erase_performance.rs:97:61` replace - with + in run
- `erase_performance.rs:143:78` replace == with != in run

### `apps/android/src/error.rs`

- `error.rs:74:9` replace AppError::kind -> Option<ErrorKind> with None

### `apps/android/src/ink_cache.rs`

- `ink_cache.rs:50:5` replace ptr -> usize with 0
- `ink_cache.rs:90:39` delete ! in InkCache::draw
- `ink_cache.rs:150:37` replace - with + in InkCache::draw
- `ink_cache.rs:216:136` replace + with - in InkCache::compute_bounds

### `apps/android/src/integration.rs`

- `integration.rs:158:78` replace * with + in Integration::live_gesture
- `integration.rs:159:74` replace + with * in Integration::live_gesture
- `integration.rs:171:45` replace < with <= in Integration::live_gesture
- `integration.rs:204:45` replace + with * in Integration::gesture
- `integration.rs:206:60` replace + with * in Integration::gesture
- `integration.rs:347:19` replace == with != in Integration::run_action
- `integration.rs:391:28` replace || with && in Integration::run_action
- `integration.rs:428:39` replace < with <= in Integration::run_action
- `integration.rs:456:54` replace || with && in Integration::run_action
- `integration.rs:487:64` replace != with == in Integration::run_action
- `integration.rs:493:114` replace != with == in Integration::run_action
- `integration.rs:515:93` replace != with == in Integration::run_action
- `integration.rs:566:99` replace == with != in Integration::run_action
- `integration.rs:606:19` replace == with != in Integration::run_action
- `integration.rs:617:105` replace || with && in Integration::run_action
- `integration.rs:617:60` replace > with == in Integration::run_action
- `integration.rs:631:46` replace > with >= in Integration::run_action
- `integration.rs:672:34` replace > with >= in Integration::run_action
- `integration.rs:695:42` replace < with == in Integration::run_action
- `integration.rs:749:19` replace == with != in Integration::run_action
- `integration.rs:799:43` replace == with != in Integration::main_actions
- `integration.rs:814:100` replace + with * in Integration::main_actions
- `integration.rs:815:89` replace * with + in Integration::main_actions
- `integration.rs:824:91` replace * with + in Integration::main_actions
- `integration.rs:875:23` replace == with != in Integration::main_actions
- `integration.rs:921:23` replace == with != in Integration::main_actions
- `integration.rs:932:127` replace * with / in Integration::main_actions
- `integration.rs:949:105` replace * with + in Integration::main_actions
- `integration.rs:951:125` replace != with == in Integration::main_actions
- `integration.rs:959:43` replace + with * in Integration::main_actions
- `integration.rs:959:145` replace * with + in Integration::main_actions
- `integration.rs:960:112` replace * with + in Integration::main_actions
- `integration.rs:961:93` replace | with & in Integration::main_actions
- `integration.rs:961:158` replace * with / in Integration::main_actions
- `integration.rs:963:39` replace - with / in Integration::main_actions
- `integration.rs:963:109` replace || with && in Integration::main_actions
- `integration.rs:980:20` delete ! in Integration::tool_switch_selection
- `integration.rs:1031:99` replace || with && in Integration::tool_switch_selection
- `integration.rs:1062:50` replace || with && in Integration::tool_switch_selection
- `integration.rs:1100:19` replace == with != in Integration::report_state
- `integration.rs:1119:39` replace - with / in Integration::report_state
- `integration.rs:1138:27` replace > with < in Integration::report_state
- `integration.rs:1180:51` replace < with <= in Integration::report_state
- `integration.rs:1182:38` replace % with / in Integration::report_state
- `integration.rs:1240:52` replace + with - in Integration::wait_until
- `integration.rs:1320:99` replace * with / in Integration::canvas_pixel
- `integration.rs:1336:28` replace * with / in Integration::images_differ
- `integration.rs:1413:12` replace > with >= in read_line_text

### `apps/android/src/interaction_performance.rs`

- `interaction_performance.rs:38:5` replace push with ()
- `interaction_performance.rs:135:142` replace / with * in run
- `interaction_performance.rs:266:67` replace && with || in run
- `interaction_performance.rs:292:28` replace == with != in run
- `interaction_performance.rs:336:83` replace + with * in run
- `interaction_performance.rs:346:54` delete ! in run
- `interaction_performance.rs:393:37` replace == with != in run
- `interaction_performance.rs:410:63` replace % with + in run
- `interaction_performance.rs:416:55` replace % with / in run
- `interaction_performance.rs:441:56` replace + with * in run
- `interaction_performance.rs:469:21` replace || with && in run
- `interaction_performance.rs:474:25` replace == with != in run
- `interaction_performance.rs:486:55` replace != with == in run
- `interaction_performance.rs:499:56` replace - with / in run
- `interaction_performance.rs:521:89` replace && with || in run
- `interaction_performance.rs:544:21` replace == with != in run

### `apps/android/src/java_port/concurrent.rs`

- `concurrent.rs:166:9` replace Future<T>::is_cancelled -> bool with false
- `concurrent.rs:194:9` replace CountDownLatch::get_count -> i64 with -1
- `concurrent.rs:205:9` replace CountDownLatch::await_timeout -> bool with true

### `apps/android/src/java_port/exception.rs`

- `exception.rs:18:9` replace JavaError::is -> bool with false
- `exception.rs:67:21` replace == with != in AppError::is
- `exception.rs:80:9` replace AppError::stack_trace -> String with String::new()

### `apps/android/src/java_port/jvm.rs`

- `jvm.rs:101:9` replace <impl fmt::Debug for Obj>::fmt -> fmt::Result with Ok(Default::default())
- `jvm.rs:213:39` delete ! in Ret::obj
- `jvm.rs:235:37` replace Ret::bool -> bool with true
- `jvm.rs:239:38` replace Ret::double -> f64 with -1.0
- `jvm.rs:242:37` replace Ret::short -> i16 with 1
- `jvm.rs:255:9` delete match arm b'I' in return_type
- `jvm.rs:393:5` replace int_array -> Vec<i32> with vec![1]
- `jvm.rs:452:5` replace unbox_float -> f32 with -1.0

### `apps/android/src/java_port/math.rs`

- `math.rs:10:5` replace round_f32 -> i32 with -1

### `apps/android/src/java_port/monitor.rs`

- `monitor.rs:58:9` replace Monitor::held_by_current_thread -> bool with true

### `apps/android/src/java_port/org_json.rs`

- `org_json.rs:18:9` replace <impl fmt::Display for JSONException>::fmt -> fmt::Result with Ok(Default::default())
- `org_json.rs:145:9` delete match arm JV::Int(i) in to_int
- `org_json.rs:155:14` replace match guard is_number(n) with false in to_long
- `org_json.rs:163:23` replace match guard s.eq_ignore_ascii_case("false") with true in to_bool
- `org_json.rs:218:9` replace <impl fmt::Debug for JSONObject>::fmt -> fmt::Result with Ok(Default::default())
- `org_json.rs:325:9` replace JSONObject::opt_double -> f64 with 0.0
- `org_json.rs:378:9` replace JSONObject::opt_json_array -> Option<JSONArray> with None
- `org_json.rs:415:9` replace <impl fmt::Debug for JSONArray>::fmt -> fmt::Result with Ok(Default::default())
- `org_json.rs:454:18` replace < with <= in JSONArray::remove
- `org_json.rs:485:9` replace JSONArray::opt_int -> i32 with 1
- `org_json.rs:505:9` replace JSONArray::opt_string -> String with "xyzzy".into()
- `org_json.rs:538:13` delete match arm JV::Arr(a) in JSONArray::from_value
- `org_json.rs:617:22` replace > with == in write_value
- `org_json.rs:722:53` replace += with -= in Tokener::next_clean
- `org_json.rs:739:24` replace < with > in Tokener::skip_to_end_of_line
- `org_json.rs:781:34` replace += with -= in Tokener::next_string
- `org_json.rs:784:158` replace <= with > in Tokener::next_string

### `apps/android/src/java_port/rect_f.rs`

- `rect_f.rs:33:21` replace - with / in <impl RectFExt for RectF>::height
- `rect_f.rs:39:9` replace <impl RectFExt for RectF>::center_y -> f32 with 0.0
- `rect_f.rs:48:9` replace <impl RectFExt for RectF>::contains_point -> bool with true
- `rect_f.rs:48:76` replace && with || in <impl RectFExt for RectF>::contains_point
- `rect_f.rs:48:116` replace < with > in <impl RectFExt for RectF>::contains_point
- `rect_f.rs:51:44` replace < with > in <impl RectFExt for RectF>::contains_rect
- `rect_f.rs:54:9` replace <impl RectFExt for RectF>::intersect -> bool with false
- `rect_f.rs:54:63` replace < with <= in <impl RectFExt for RectF>::intersect
- `rect_f.rs:55:26` replace < with > in <impl RectFExt for RectF>::intersect
- `rect_f.rs:64:28` replace > with >= in <impl RectFExt for RectF>::intersect
- `rect_f.rs:75:21` replace += with -= in <impl RectFExt for RectF>::offset

### `apps/android/src/java_port/uri.rs`

- `uri.rs:36:5` replace is_mark -> bool with true
- `uri.rs:48:5` replace is_pchar -> bool with true
- `uri.rs:63:23` replace == with != in is_server_percent
- `uri.rs:128:14` replace > with < in Parser::scan_escape
- `uri.rs:138:27` replace < with <= in Parser::scan
- `uri.rs:306:14` replace < with > in Parser::parse_server
- `uri.rs:402:42` replace > with >= in Parser::parse_ipv6_reference
- `uri.rs:483:18` replace > with >= in Parser::scan_hex_seq

### `apps/android/src/jx.rs`

- `jx.rs:40:9` replace IdCache::hash -> u64 with 0
- `jx.rs:49:58` replace == with != in IdCache::get
- `jx.rs:171:13` delete match arm ReturnType::Primitive(Primitive::Void) in call_nonvirtual
- `jx.rs:228:9` delete match arm ReturnType::Primitive(p) in get_static
- `jx.rs:252:13` delete match arm b'I' in set_field
- `jx.rs:305:5` replace throw_to_java with ()
- `jx.rs:518:15` replace != with == in release
- `jx.rs:626:53` replace log::e with ()

### `apps/android/src/lib.rs`

- `lib.rs:56:5` replace JNI_OnLoad -> jint with Default::default()
- `lib.rs:74:5` replace reply_to_java -> jobject with Default::default()

### `apps/android/src/main_activity.rs`

- `main_activity.rs:201:5` replace argv -> Vec<&str> with vec![]
- `main_activity.rs:373:9` replace <impl SharedState for State>::map -> &ReplicaState with Box::leak(Box::new(Default::default()))
- `main_activity.rs:836:37` replace == with != in MainActivity::on_create
- `main_activity.rs:1000:35` replace * with / in MainActivity::on_stop
- `main_activity.rs:1019:121` replace || with && in MainActivity::on_activity_result
- `main_activity.rs:1053:32` replace + with * in MainActivity::import_picture
- `main_activity.rs:1134:66` replace && with || in MainActivity::persist
- `main_activity.rs:1163:91` replace >= with < in MainActivity::persist
- `main_activity.rs:1169:41` replace != with == in MainActivity::persist
- `main_activity.rs:1174:43` replace != with == in MainActivity::persist
- `main_activity.rs:1235:103` replace > with < in MainActivity::place_text
- `main_activity.rs:1238:69` replace * with / in MainActivity::place_text
- `main_activity.rs:1288:9` replace MainActivity::same_painted_object -> bool with false
- `main_activity.rs:1295:31` replace != with == in MainActivity::same_painted_object
- `main_activity.rs:1369:73` replace += with -= in MainActivity::persist_object
- `main_activity.rs:1440:36` replace && with || in MainActivity::save
- `main_activity.rs:1449:17` replace < with > in MainActivity::refresh_shared
- `main_activity.rs:1487:35` delete ! in MainActivity::refresh_shared
- `main_activity.rs:1498:60` replace > with == in MainActivity::refresh_shared
- `main_activity.rs:1598:93` replace == with != in MainActivity::load
- `main_activity.rs:1622:47` replace += with *= in MainActivity::apply_with
- `main_activity.rs:1749:90` replace || with && in MainActivity::save_network_settings
- `main_activity.rs:1829:83` delete ! in MainActivity::history
- `main_activity.rs:1909:29` replace && with || in MainActivity::apply_refresh
- `main_activity.rs:1923:53` replace > with >= in MainActivity::apply_refresh
- `main_activity.rs:1981:48` replace || with && in MainActivity::local_loop
- `main_activity.rs:2018:17` replace < with == in MainActivity::start_cli
- `main_activity.rs:2147:24` delete ! in MainActivity::object_command
- `main_activity.rs:2266:141` replace && with || in MainActivity::start_selection
- `main_activity.rs:2316:13` replace / with % in MainActivity::hit_radius
- `main_activity.rs:2339:9` replace MainActivity::add_lasso_point with ()
- `main_activity.rs:2352:26` replace < with <= in MainActivity::select_lasso
- `main_activity.rs:2367:61` replace && with || in MainActivity::selected_entity_at
- `main_activity.rs:2387:31` replace match guard AppError::from(stale.clone()).is("java.lang.IllegalArgumentException") with true in MainActivity::selected_entity_at
- `main_activity.rs:2410:87` delete ! in MainActivity::deselect_entity
- `main_activity.rs:2431:67` replace != with == in MainActivity::deselect_entity
- `main_activity.rs:2465:41` replace || with && in MainActivity::schedule_heartbeat
- `main_activity.rs:2503:26` replace > with == in MainActivity::schedule_preview_expiry
- `main_activity.rs:2520:47` replace && with || in MainActivity::deselect
- `main_activity.rs:2568:55` replace || with && in MainActivity::preview_transform
- `main_activity.rs:2600:65` replace && with || in MainActivity::dispatch_latest_preview
- `main_activity.rs:2681:24` delete ! in MainActivity::commit_transform_preview
- `main_activity.rs:2743:24` delete ! in MainActivity::resolve_conflict
- `main_activity.rs:2892:36` delete - in MainActivity::selection_command
- `main_activity.rs:2951:12` delete ! in MainActivity::apply_erase
- `main_activity.rs:3071:42` replace / with % in MainActivity::placed_object
- `main_activity.rs:3087:65` replace / with % in MainActivity::insert_svg_template
- `main_activity.rs:3102:75` replace * with + in MainActivity::insert_gif
- `main_activity.rs:3148:84` replace < with == in MainActivity::text_placement_dialog
- `main_activity.rs:3467:9` replace CanvasView::width -> ui::Px with Default::default()
- `main_activity.rs:3503:13` delete match arm "onDetachedFromWindow()" in CanvasView::dispatch
- `main_activity.rs:3521:44` replace * with / in CanvasView::on_scale
- `main_activity.rs:3526:35` replace - with / in CanvasView::on_scale
- `main_activity.rs:3539:61` replace && with || in CanvasView::pending_erase
- `main_activity.rs:3548:9` replace CanvasView::x -> f32 with 1.0
- `main_activity.rs:3552:12` replace - with + in CanvasView::y
- `main_activity.rs:3564:66` replace && with || in CanvasView::visible
- `main_activity.rs:3587:64` replace * with / in CanvasView::draw_infinite_template
- `main_activity.rs:3591:35` replace / with * in CanvasView::draw_infinite_template
- `main_activity.rs:3602:20` replace == with != in CanvasView::draw_infinite_template
- `main_activity.rs:3612:19` replace += with -= in CanvasView::draw_infinite_template
- `main_activity.rs:3672:177` replace && with || in CanvasView::settled_ink
- `main_activity.rs:3697:166` replace && with || in CanvasView::settled_ink
- `main_activity.rs:3729:97` replace && with || in CanvasView::settled_ink
- `main_activity.rs:3797:48` replace || with && in CanvasView::on_draw
- `main_activity.rs:3853:16` delete ! in CanvasView::on_draw
- `main_activity.rs:3876:121` replace == with != in CanvasView::on_draw
- `main_activity.rs:3892:96` replace && with || in CanvasView::on_draw
- `main_activity.rs:3897:69` replace && with || in CanvasView::on_draw
- `main_activity.rs:3902:180` replace && with || in CanvasView::on_draw
- `main_activity.rs:3942:41` replace && with || in CanvasView::on_draw
- `main_activity.rs:3967:45` replace - with + in CanvasView::on_draw
- `main_activity.rs:3981:24` delete ! in CanvasView::on_draw
- `main_activity.rs:4105:25` replace * with / in CanvasView::inverse_preview_viewport
- `main_activity.rs:4111:26` replace + with - in CanvasView::inverse_preview_viewport
- `main_activity.rs:4112:38` replace * with + in CanvasView::inverse_preview_viewport
- `main_activity.rs:4113:33` replace + with - in CanvasView::inverse_preview_viewport
- `main_activity.rs:4188:57` replace + with * in CanvasView::draw_transform_previews
- `main_activity.rs:4297:95` replace > with >= in CanvasView::point
- `main_activity.rs:4297:68` replace < with <= in CanvasView::point
- `main_activity.rs:4325:69` replace >= with < in CanvasView::erase_point
- `main_activity.rs:4346:9` replace CanvasView::scribble -> bool with true
- `main_activity.rs:4362:29` replace - with / in CanvasView::scribble
- `main_activity.rs:4365:33` replace > with < in CanvasView::scribble
- `main_activity.rs:4373:38` replace && with || in CanvasView::scribble
- `main_activity.rs:4381:52` replace >= with < in CanvasView::scribble
- `main_activity.rs:4389:37` replace == with != in CanvasView::on_touch_event
- `main_activity.rs:4411:55` replace + with - in CanvasView::on_touch_event
- `main_activity.rs:4432:35` replace == with != in CanvasView::on_touch_event
- `main_activity.rs:4447:61` replace - with + in CanvasView::on_touch_event
- `main_activity.rs:4491:37` replace < with <= in CanvasView::on_touch_event
- `main_activity.rs:4491:61` replace > with >= in CanvasView::on_touch_event
- `main_activity.rs:4533:55` replace - with + in CanvasView::on_touch_event
- `main_activity.rs:4563:49` replace && with || in CanvasView::on_touch_event
- `main_activity.rs:4608:40` replace && with || in CanvasView::on_touch_event
- `main_activity.rs:4616:56` replace > with >= in CanvasView::on_touch_event
- `main_activity.rs:4635:55` replace - with + in CanvasView::on_touch_event
- `main_activity.rs:4656:26` replace == with != in CanvasView::on_touch_event
- `main_activity.rs:4670:36` replace + with * in CanvasView::on_touch_event
- `main_activity.rs:4692:25` replace != with == in CanvasView::on_touch_event

### `apps/android/src/performance.rs`

- `performance.rs:63:5` replace sleep with ()
- `performance.rs:170:72` replace + with * in position_xy
- `performance.rs:176:40` replace * with / in position_xy
- `performance.rs:289:81` replace / with % in prewarm_graphics
- `performance.rs:318:5` replace values -> Vec<f64> with vec![1.0]
- `performance.rs:374:110` replace / with % in run
- `performance.rs:410:35` replace && with || in run
- `performance.rs:458:70` replace == with != in run
- `performance.rs:481:43` replace + with - in run
- `performance.rs:486:75` replace / with * in run
- `performance.rs:515:69` replace - with + in run
- `performance.rs:542:23` replace % with / in run
- `performance.rs:570:44` replace == with != in run
- `performance.rs:686:80` replace == with != in proc_io

### `apps/android/src/replica_service.rs`

- `replica_service.rs:64:80` replace < with > in open_proxy

### `apps/android/src/stroke_check.rs`

- `stroke_check.rs:57:74` replace * with / in input

### `apps/android/src/stroke_crossings.rs`

- `stroke_crossings.rs:54:46` replace * with + in seams
- `stroke_crossings.rs:59:30` replace == with != in seams
- `stroke_crossings.rs:68:48` replace + with - in seams
- `stroke_crossings.rs:119:80` replace + with * in run
- `stroke_crossings.rs:133:47` replace / with % in run

### `apps/android/src/tile_ink_cache.rs`

- `tile_ink_cache.rs:36:31` replace * with +
- `tile_ink_cache.rs:67:5` replace get -> i64 with -1
- `tile_ink_cache.rs:134:9` replace <impl From<&BoundsCacheMetrics> for Value>::from -> Value with Default::default()
- `tile_ink_cache.rs:460:43` replace < with <= in TileInkCache::tier_for
- `tile_ink_cache.rs:469:45` replace - with / in TileInkCache::touch_tile
- `tile_ink_cache.rs:501:22` replace <= with > in TileInkCache::draw_tiles
- `tile_ink_cache.rs:520:58` replace - with / in TileInkCache::draw_one_tile
- `tile_ink_cache.rs:530:21` replace && with || in TileInkCache::draw_one_tile
- `tile_ink_cache.rs:554:43` replace * with / in TileInkCache::draw_one_tile
- `tile_ink_cache.rs:614:44` replace / with % in TileInkCache::draw_monolithic
- `tile_ink_cache.rs:619:28` replace * with + in TileInkCache::draw_monolithic
- `tile_ink_cache.rs:622:30` replace - with + in TileInkCache::draw_monolithic
- `tile_ink_cache.rs:623:39` replace * with + in TileInkCache::draw_monolithic
- `tile_ink_cache.rs:644:28` replace - with / in TileInkCache::shader
- `tile_ink_cache.rs:652:21` replace * with / in TileInkCache::draw_tile
- `tile_ink_cache.rs:657:21` replace * with / in TileInkCache::draw_tile_bitmap
- `tile_ink_cache.rs:681:16` delete ! in TileInkCache::same
- `tile_ink_cache.rs:689:45` replace > with < in TileInkCache::trim_for
- `tile_ink_cache.rs:716:28` replace > with < in TileInkCache::draw_item
- `tile_ink_cache.rs:754:34` replace * with / in TileInkCache::draw_item
- `tile_ink_cache.rs:765:59` replace - with / in TileInkCache::draw_item
- `tile_ink_cache.rs:768:130` replace && with || in TileInkCache::draw_item
- `tile_ink_cache.rs:833:9` replace TileInkCache::trim_paths with ()
- `tile_ink_cache.rs:833:98` replace > with == in TileInkCache::trim_paths
- `tile_ink_cache.rs:842:9` replace TileInkCache::remove_path with ()
- `tile_ink_cache.rs:959:60` replace > with == in TileInkCache::bounds
- `tile_ink_cache.rs:974:136` replace + with * in TileInkCache::compute_bounds

### `apps/android/src/ui.rs`

- `ui.rs:118:29` replace * with / in dp

### `apps/android/src/ui_actions.rs`

- `ui_actions.rs:51:32` replace <= with > in trim

### `apps/android/src/viewport_check.rs`

- `viewport_check.rs:56:29` replace match guard n.is_f64() with true in list_string
- `viewport_check.rs:223:15` replace < with <= in nearby_tile_boundary
- `viewport_check.rs:238:95` replace - with + in seam_compare
- `viewport_check.rs:245:36` replace * with + in seam_compare
- `viewport_check.rs:247:25` replace - with + in seam_compare
- `viewport_check.rs:259:25` replace += with -= in seam_compare
- `viewport_check.rs:269:66` replace - with + in seam_compare
- `viewport_check.rs:308:23` replace == with != in seam_compare
- `viewport_check.rs:325:20` replace - with / in edge_hits
- `viewport_check.rs:337:26` replace += with *= in edge_hits
- `viewport_check.rs:360:37` replace > with >= in run
- `viewport_check.rs:372:41` replace + with * in run
- `viewport_check.rs:374:19` delete - in run
- `viewport_check.rs:375:62` replace + with * in run
- `viewport_check.rs:384:35` replace == with != in run
- `viewport_check.rs:399:66` replace + with * in run
- `viewport_check.rs:419:44` replace || with && in run
- `viewport_check.rs:472:92` replace || with && in run
- `viewport_check.rs:478:73` replace + with - in run
- `viewport_check.rs:479:60` replace * with / in run
- `viewport_check.rs:483:32` replace > with >= in run
- `viewport_check.rs:488:48` replace && with || in run
- `viewport_check.rs:499:40` replace > with == in run
- `viewport_check.rs:533:35` replace > with == in run
- `viewport_check.rs:542:63` replace + with - in run
- `viewport_check.rs:602:35` replace > with < in run
- `viewport_check.rs:621:68` replace * with + in run
- `viewport_check.rs:628:35` replace > with < in run

### `apps/cli/src/bin/cache_footprint_probe.rs`

- `cache_footprint_probe.rs:9:5` replace stroke -> Map with Default::default()
- `cache_footprint_probe.rs:21:36` delete - in run
- `cache_footprint_probe.rs:21:5` replace run -> Result<()> with Ok(())

### `apps/cli/src/bin/coverage_parity.rs`

- `coverage_parity.rs:20:22` replace << with >>
- `coverage_parity.rs:32:36` replace - with + in JavaRandom::next
- `coverage_parity.rs:38:36` replace / with % in JavaRandom::next_double
- `coverage_parity.rs:90:52` replace * with + in run
- `coverage_parity.rs:105:18` replace == with != in run

### `apps/cli/src/bin/latency_probe.rs`

- `latency_probe.rs:77:22` replace == with != in run
- `latency_probe.rs:166:28` replace < with > in run
- `latency_probe.rs:166:69` replace < with > in run
- `latency_probe.rs:179:17` replace != with == in run

### `apps/cli/src/bin/stroke_svg_raster.rs`

- `stroke_svg_raster.rs:7:5` replace run -> Result<(), String> with Ok(())
- `stroke_svg_raster.rs:23:16` replace - with + in run
- `stroke_svg_raster.rs:24:16` replace - with + in run

### `apps/cli/src/canvas.rs`

- `canvas.rs:117:9` replace Path::fill_type -> FillType with Default::default()
- `canvas.rs:150:13` delete match arm None in Path::inject_move
- `canvas.rs:186:49` replace * with + in Path::add_oval
- `canvas.rs:191:52` replace - with + in Path::add_oval
- `canvas.rs:337:51` replace + with * in Path::flatten
- `canvas.rs:344:68` replace * with + in Path::flatten
- `canvas.rs:352:45` replace * with + in Path::flatten
- `canvas.rs:354:40` replace * with + in Path::flatten
- `canvas.rs:359:47` replace * with / in Path::flatten
- `canvas.rs:359:81` replace * with / in Path::flatten
- `canvas.rs:361:45` replace * with + in Path::flatten
- `canvas.rs:362:41` replace + with * in Path::flatten
- `canvas.rs:477:9` replace Paint::anti_alias -> Paint with Default::default()
- `canvas.rs:527:27` replace + with * in draw_svg_raster

### `apps/cli/src/cli_main.rs`

- `cli_main.rs:285:30` replace + with * in image_placement
- `cli_main.rs:459:55` replace == with != in run_command
- `cli_main.rs:604:25` replace && with || in run_command
- `cli_main.rs:782:25` replace && with || in run_command
- `cli_main.rs:812:31` replace == with != in run_command
- `cli_main.rs:880:21` replace && with || in run_command
- `cli_main.rs:1065:33` replace == with != in selection
- `cli_main.rs:1164:31` replace && with || in entity_command

### `apps/cli/src/item_coverage.rs`

- `item_coverage.rs:214:62` replace && with || in rect_hits
- `item_coverage.rs:260:42` replace != with == in ItemCoverage::paint
- `item_coverage.rs:372:22` replace == with != in <impl Surface for ItemCoverage>::rect
- `item_coverage.rs:417:37` replace == with != in <impl Surface for ItemCoverage>::styled_text

### `apps/cli/src/java_port/jutil.rs`

- `jutil.rs:37:28` replace match guard t == me with true in Monitor::enter

### `apps/cli/src/local_proxy.rs`

- `local_proxy.rs:722:9` replace LocalProxy::version -> i64 with -1
- `local_proxy.rs:738:39` replace + with - in LocalProxy::await_change_timeout
- `local_proxy.rs:790:12` delete ! in LocalProxy::preempt_fetch_sweep
- `local_proxy.rs:1154:17` replace || with && in LocalProxy::sync_once
- `local_proxy.rs:1217:36` replace * with / in LocalProxy::subscribe
- `local_proxy.rs:1277:20` delete ! in LocalProxy::ui_loop
- `local_proxy.rs:1842:28` replace >= with < in LocalProxy::execute_value
- `local_proxy.rs:1875:48` replace == with != in LocalProxy::execute_value
- `local_proxy.rs:2069:65` replace == with != in LocalProxy::execute_value
- `local_proxy.rs:2114:23` replace > with >= in LocalProxy::forward_selection_change
- `local_proxy.rs:2144:48` replace > with < in LocalProxy::forward_selection_change
- `local_proxy.rs:2150:17` replace && with || in LocalProxy::forward_selection_change
- `local_proxy.rs:2269:97` replace && with || in LocalProxy::snapshot_state
- `local_proxy.rs:2357:36` replace + with * in LocalProxy::acquire_selection
- `local_proxy.rs:2524:32` replace == with != in LocalProxy::release
- `local_proxy.rs:2572:21` replace == with != in LocalProxy::release_actor
- `local_proxy.rs:2642:9` replace LocalProxy::heartbeat_selection -> Result<Map> with Ok(Default::default())
- `local_proxy.rs:2701:36` delete ! in LocalProxy::heartbeat_selection
- `local_proxy.rs:2867:35` replace > with < in uploads_changes
- `local_proxy.rs:2880:17` replace && with || in uploads_changes
- `local_proxy.rs:3004:63` replace + with - in Uri::path
- `local_proxy.rs:3100:94` replace || with && in apply_remote_leases
- `local_proxy.rs:3115:181` replace || with && in apply_remote_leases

### `apps/cli/src/png.rs`

- `png.rs:107:5` replace svgs -> &'static Mutex<IndexMap<SvgKey, Arc<SvgTree>>> with Box::leak(Box::new(Mutex::new(Default::default())))
- `png.rs:134:5` replace ms -> String with "xyzzy".into()
- `png.rs:186:29` replace - with + in decode_jpeg
- `png.rs:205:45` replace + with - in decode_gif
- `png.rs:210:25` replace * with + in decode_gif
- `png.rs:252:12` replace > with == in fit_root_size
- `png.rs:280:13` delete field cap from struct Paint expression in Png<'a>::base_paint
- `png.rs:409:9` replace Png<'a>::draw_dots with ()
- `png.rs:413:21` replace - with + in Png<'a>::draw_dots
- `png.rs:423:13` delete field color from struct Paint expression in Png<'a>::draw_dots
- `png.rs:432:29` replace - with / in Png<'a>::draw_dots
- `png.rs:436:41` replace + with - in Png<'a>::draw_dots
- `png.rs:499:32` replace + with * in Png<'a>::cached_image
- `png.rs:501:17` replace && with || in Png<'a>::cached_image
- `png.rs:646:29` replace == with != in <impl Surface for Png<'_>>::rect
- `png.rs:707:28` replace == with != in <impl Surface for Png<'_>>::styled_text
- `png.rs:731:50` replace + with - in <impl Surface for Png<'_>>::styled_text

### `apps/cli/src/probe.rs`

- `probe.rs:35:5` replace allocated_bytes -> i64 with 1
- `probe.rs:86:55` replace / with % in ms_since

### `apps/cli/src/raster.rs`

- `raster.rs:79:21` replace | with & in RasterCanvas::argb_pixels
- `raster.rs:142:43` replace > with == in <impl Canvas for RasterCanvas>::restore_to_count
- `raster.rs:185:50` replace + with - in <impl Canvas for RasterCanvas>::clip_out_path

### `apps/cli/src/typeface.rs`

- `typeface.rs:138:13` replace && with || in load
- `typeface.rs:246:53` replace * with / in Typeface::text_path
- `typeface.rs:300:56` replace - with + in fontconfig_dirs

### `apps/cli/src/ui_hub.rs`

- `ui_hub.rs:62:5` replace now_millis -> i64 with -1
- `ui_hub.rs:73:5` replace expires -> i64 with 0
- `ui_hub.rs:163:61` replace + with - in UiHub::heartbeat
- `ui_hub.rs:225:33` replace match guard json::jlen(s) <= 200 with true in UiHub::command

### `apps/server/server.py`

- `server.py:59` replace or with and
- `server.py:80` replace or with and
- `server.py:290` delete call statement
- `server.py:477` replace == with !=
- `server.py:499` replace and with or
- `server.py:571` delete call statement

### `apps/server/svg_asset.py`

- `svg_asset.py:51` replace == with !=

### `apps/server/ui_sessions.py`

- `ui_sessions.py:13` replace __init__ body with return None
- `ui_sessions.py:33` replace <= with >
- `ui_sessions.py:46` delete not
- `ui_sessions.py:65` replace == with !=
- `ui_sessions.py:88` replace command body with return None
- `ui_sessions.py:100` delete call statement

### `packages/core/src/checkpoint_point_prefixes.rs`

- `checkpoint_point_prefixes.rs:77:26` replace < with <= in Encoder::operation
- `checkpoint_point_prefixes.rs:82:19` replace < with <= in Encoder::operation

### `packages/core/src/collaboration.rs`

- `collaboration.rs:183:67` replace && with || in add
- `collaboration.rs:369:71` replace && with || in transform
- `collaboration.rs:420:71` replace && with || in transform
- `collaboration.rs:466:79` replace ^ with | in transform

### `packages/core/src/coverage.rs`

- `coverage.rs:82:24` replace > with == in Tiles::tile
- `coverage.rs:108:33` replace - with + in <impl Coverage for Tiles>::intersects

### `packages/core/src/draw_item.rs`

- `draw_item.rs:28:5` replace prepared_cache -> &'static Mutex<PreparedCache> with Box::leak(Box::new(Mutex::new(Default::default())))
- `draw_item.rs:174:23` replace == with != in DrawItem::coverage
- `draw_item.rs:254:61` delete - in DrawItem::intersects
- `draw_item.rs:259:60` replace * with + in DrawItem::intersects

### `packages/core/src/drawing_actions.rs`

- `drawing_actions.rs:311:17` replace < with <= in template
- `drawing_actions.rs:350:25` delete - in template

### `packages/core/src/endpoint.rs`

- `endpoint.rs:393:25` replace == with != in HttpConnection::connect

### `packages/core/src/geometry.rs`

- `geometry.rs:32:9` replace DocRect::origin -> DocPoint with Default::default()
- `geometry.rs:135:61` replace < with <= in Polygon::intersects
- `geometry.rs:156:45` replace < with <= in Polygon::intersects
- `geometry.rs:161:77` replace * with / in Polygon::intersects

### `packages/core/src/gif_asset.rs`

- `gif_asset.rs:6:32` replace * with +
- `gif_asset.rs:93:54` replace && with || in validate_bytes
- `gif_asset.rs:131:41` replace + with * in validate_bytes

### `packages/core/src/ids.rs`

- `ids.rs:221:16` replace == with != in EntityKind::is_stroke

### `packages/core/src/image_asset.rs`

- `image_asset.rs:7:32` replace * with +
- `image_asset.rs:58:47` replace - with / in validate
- `image_asset.rs:92:33` replace && with || in validate
- `image_asset.rs:102:24` replace > with >= in validate

### `packages/core/src/java_port/file.rs`

- `file.rs:8:5` replace canonical_path -> String with "xyzzy".into()
- `file.rs:18:11` delete ! in canonical_path

### `packages/core/src/java_port/jfmt.rs`

- `jfmt.rs:253:64` replace < with <= in Buffer::dtoa
- `jfmt.rs:307:42` replace > with == in Buffer::dtoa
- `jfmt.rs:314:42` replace || with && in Buffer::dtoa
- `jfmt.rs:322:26` replace > with >= in Buffer::dtoa
- `jfmt.rs:487:15` replace && with || in insignificant_digits_for_pow2
- `jfmt.rs:509:20` replace & with | in estimate_dec_exp

### `packages/core/src/java_port/jmath.rs`

- `jmath.rs:67:22` replace > with == in atan
- `jmath.rs:100:21` delete - in atan
- `jmath.rs:118:59` replace > with >= in atan2
- `jmath.rs:137:15` replace == with != in atan2
- `jmath.rs:141:35` replace + with - in atan2
- `jmath.rs:149:22` delete - in atan2
- `jmath.rs:159:18` replace < with > in atan2
- `jmath.rs:178:22` delete - in acos
- `jmath.rs:193:29` replace - with / in acos
- `jmath.rs:200:26` replace + with - in acos
- `jmath.rs:200:59` replace + with - in acos
- `jmath.rs:201:32` replace + with - in acos
- `jmath.rs:202:19` replace / with * in acos
- `jmath.rs:205:22` replace + with * in acos
- `jmath.rs:206:74` replace * with + in acos
- `jmath.rs:206:41` replace * with + in acos
- `jmath.rs:207:47` replace * with + in acos
- `jmath.rs:211:12` replace - with + in acos
- `jmath.rs:217:37` replace + with * in acos
- `jmath.rs:217:70` replace + with * in acos
- `jmath.rs:218:43` replace + with * in acos
- `jmath.rs:220:23` replace + with * in acos
- `jmath.rs:230:14` replace == with != in hypot
- `jmath.rs:241:18` replace + with - in hypot
- `jmath.rs:250:11` replace += with *= in hypot
- `jmath.rs:262:15` replace *= with /= in hypot
- `jmath.rs:269:15` replace -= with /= in hypot
- `jmath.rs:293:5` replace power_of_two -> f64 with 0.0
- `jmath.rs:321:22` replace < with <= in round
- `jmath.rs:364:54` replace || with && in min

### `packages/core/src/java_port/json.rs`

- `json.rs:296:84` delete - in Decimal::cmp
- `json.rs:340:22` replace > with < in append_java_string
- `json.rs:684:40` replace < with > in Parser<'a>::str
- `json.rs:819:31` replace > with >= in java_hash_set_order

### `packages/core/src/journal.rs`

- `journal.rs:332:83` replace != with == in Journal::access_locked
- `journal.rs:363:24` replace < with <= in Journal::access_locked
- `journal.rs:571:38` delete ! in Journal::apply_with
- `journal.rs:671:21` replace += with *= in Journal::append_edit
- `journal.rs:898:5` replace identity -> Option<usize> with None
- `journal.rs:916:14` replace == with != in same_actors

### `packages/core/src/model.rs`

- `model.rs:37:9` replace Shared<T>::ptr_eq -> bool with false
- `model.rs:62:51` replace == with != in <impl PartialEq for Shared<T>>::eq
- `model.rs:744:5` replace input_color -> Result<()> with Ok(())

### `packages/core/src/net.rs`

- `net.rs:86:9` replace <impl From<String> for BearerToken>::from -> BearerToken with Default::default()
- `net.rs:186:9` replace <impl From<HttpStatus> for u16>::from -> u16 with 1

### `packages/core/src/region_fill.rs`

- `region_fill.rs:55:27` replace && with || in create
- `region_fill.rs:125:55` replace - with + in create
- `region_fill.rs:139:54` replace - with + in create
- `region_fill.rs:143:66` replace / with * in create
- `region_fill.rs:152:55` replace - with / in create
- `region_fill.rs:159:22` replace - with + in create
- `region_fill.rs:198:38` replace - with / in create
- `region_fill.rs:210:25` replace - with / in create
- `region_fill.rs:217:26` replace + with - in create

### `packages/core/src/render.rs`

- `render.rs:27:9` replace Surface::begin_erase -> Result<()> with Ok(())
- `render.rs:230:27` replace < with <= in stroke_path
- `render.rs:273:36` replace * with / in fill_ribbon
- `render.rs:289:37` replace > with >= in fill_ribbon
- `render.rs:732:37` replace < with == in interior_overlap
- `render.rs:735:41` replace < with == in interior_overlap
- `render.rs:743:47` replace < with == in interior_overlap
- `render.rs:743:116` replace > with == in interior_overlap
- `render.rs:763:94` replace / with % in interior_overlap
- `render.rs:763:44` replace - with + in interior_overlap

### `packages/core/src/store.rs`

- `store.rs:369:38` replace && with || in Store::open
- `store.rs:512:35` replace && with || in Store::delete_document
- `store.rs:735:60` replace > with >= in Store::page
- `store.rs:910:24` replace += with -= in Store::sync

### `packages/core/src/style.rs`

- `style.rs:15:104` replace & with | in ArgbColor::channels

### `packages/core/src/svg_asset.rs`

- `svg_asset.rs:112:9` delete - in after
- `svg_asset.rs:125:14` replace < with == in decimal
- `svg_asset.rs:158:17` replace < with <= in decimal
- `svg_asset.rs:173:10` replace < with == in match_decimal
- `svg_asset.rs:247:48` replace || with && in dot_root
- `svg_asset.rs:284:21` replace < with == in generated_dots
- `svg_asset.rs:292:27` replace < with == in generated_dots
- `svg_asset.rs:296:21` replace < with == in generated_dots
- `svg_asset.rs:301:57` replace > with >= in generated_dots
- `svg_asset.rs:436:99` replace || with && in Tokenizer<'a>::document
- `svg_asset.rs:708:10` replace < with <= in match_local_url
- `svg_asset.rs:961:67` replace == with != in write_cdata
- `svg_asset.rs:985:28` replace match guard i > 0 with true in serialize_element

### `packages/core/src/timing.rs`

- `timing.rs:14:5` replace set_observer -> Option<Observer> with None
- `timing.rs:23:5` replace mark with ()

### `packages/core/src/tls.rs`

- `tls.rs:42:9` replace <impl std::fmt::Debug for TlsConfig>::fmt -> std::fmt::Result with Ok(Default::default())

### `packages/core/src/wire.rs`

- `wire.rs:344:9` replace Entity::ptr_eq -> bool with false

## Caught only by a timeout

- `packages/core/src/endpoint.rs:575:9: replace Endpoint::cancel -> bool with false`
- `apps/android/src/java_port/uri.rs:144:22: replace > with >= in Parser::scan`

## Flaky in baseline (excluded)

- `py:selection_checks:SelectionChecks.test_cross_proxy_preview_clears_on_deselect_and_commit`

## Failures dropped by the confirmation rerun

- `py:selection_checks:SelectionChecks.test_cross_proxy_preview_clears_on_deselect_and_commit`: 50
- `py:chat_checks:ChatChecks.test_reuse_agent_proxy_after_server_restart`: 4
- `py:drawing_browser:DrawingBrowser.test_cli_synced_shapes_render_as_shapes`: 3
- `py:cli_checks:CliChecks.test_edit_during_sync_and_lost_ack`: 2
- `py:chat_checks:ChatChecks.test_history_pagination_and_restart`: 1
- `py:chat_checks:ChatChecks.test_sandbox_cli_and_network`: 1
- `py:framework_checks:NetworkHarnessChecks)`: 1
- `py:chat_checks:ChatChecks.test_chat_api_forwards_through_every_proxy_hop`: 1
- `py:topology_properties:TopologyProperties.test_batch_deadline_rate_bounds_and_idle`: 1
- `py:proxy_checks:ProxyChecks)`: 1
- `py:cli_checks:CliChecks.test_deletion_index_is_atomic_and_retains_log [from server_checks.ServerChecks]`: 1
- `py:topology_properties:TopologyProperties)`: 1
- `py:topology_properties:TopologyProperties.test_generated_trees_converge`: 1
- `py:topology_properties:TopologyProperties.test_deletion_index_is_atomic_and_retains_log [from server_checks.ServerChecks]`: 1
