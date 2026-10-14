# ADR 0002: Tool rail and collapsible layer tray

Status: accepted, 2026-10-10.

The permanent layer tray, selected-tool background, and original rail grouping
are superseded by [ADR 0010](0010-anchored-editor-panels.md). Canvas stability,
native vector icons and local-state editing remain applicable.

## Context

The initial editor's two rows of full-width buttons took attention away from the
page and placed layer visibility inside a dialog. The editor needs a tool sidebar
with contextual actions and a visible layer diagram on the opposite side.

## Decision

`EditorUi` owns native Android editor controls. A narrow left rail groups document,
pen, eraser, navigation, history, and page actions. Tapping a tool opens an anchored
popup with its common actions. Pen colors and width can be changed directly in the
popup. Tool selection has a high-contrast background and icons also have labels
and accessibility descriptions.

The right layer tray displays layers from top to bottom in their compositing
order. Each card includes a stroke preview, layer name, visible/hidden state,
selection state, and an eye button. Its menu provides shared force visibility and
selection. Forced layers display a lock and “For everyone.” Hidden previews have
a slash as well as a textual state, so meaning does not depend on color.

The tray switches immediately between a named-card layout and a compact column
of previews and visibility controls. Its expansion preference persists locally.
It overlays the drawing view; the canvas bounds, coordinate mapping, pan, and zoom
stay fixed when the tray opens or closes. The tray consumes touches in its empty
areas. Users can collapse it to reveal the covered part of the page.

Use vector icons, neutral surfaces, clear outlines, and no popup/tray transition
animation for E Ink. Layer cards rebuild when their metadata changes; stroke
preview redraws are throttled during drawing. Unchanged header text and selected
tool styling do not trigger repeated updates.

## Verification

`tests/ui_review.py` uses actual Android UI controls on a running API 36 emulator.
It checks popup actions, colored drawing, creating/selecting layers, hiding and
forcing visibility, undo/redo, stable canvas bounds across tray changes, touch
isolation, and expansion preference after restart. Review screenshots are saved
under `.test-artifacts/ui-review/`. Real BOOX refresh and stylus behavior still
require hardware validation.
