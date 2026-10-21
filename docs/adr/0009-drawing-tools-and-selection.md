# ADR 0009: Drawing tools through the shared proxy

Status: accepted, 2026-10-13; expanded implementation and verification through 2026-10-21.

## Context

Expand NoteMaker's drawing tools using the extracted BOOX Notes app as a
behavioral reference. Preserve our application boundaries, existing notes,
diagrams, SVG assets, actor histories, and offline proxy replicas. Assets and
icons are original, with monochrome E Ink presentation and color where it
communicates a pen or fill choice. Handwriting-to-text is deferred.

## Decision

All document actions enter the selected proxy, update its durable local store,
and synchronize through the existing proxy tree to the main server. Android
and CLI use the same commands and validation. A shape remains an editable
object rather than being flattened to strokes. Android extends the existing
editor chrome with grouped panels. Their current organization and styling are
specified in [ADR 0010](0010-anchored-editor-panels.md).

The accepted feature scope includes pens and presets, geometric shapes and
3D figure drawings, text boxes, styling and enclosed-area fill, selection and
transforms, erasing, layer management, pictures, templates, canvas controls,
and handwriting gestures such as scribble-to-erase. Recognition into text is
not part of the current implementation scope.

Selection collaboration uses object/stroke locks shared by Android and
CLI/agent sessions. Selection skips locked entities and reports what was
skipped. Undo and bulk operations that affect locked entities fail as a whole.
Deselecting or cleanly disconnecting releases locks; missing heartbeats expire
locks after 15 seconds. Layers are not implicitly locked by selecting an object.
Choosing another tool ends the selection, cancels any uncommitted transform
preview and releases its leases through the selected proxy. Delayed acquisition
responses cannot restore a selection after its tool or document context ends.

Transforms send ephemeral previews through proxies. Only the completed
transform becomes a durable atomic action and one undo entry. When a lock
expires, remote previews disappear and the last committed appearance returns.
Lease-only notifications refresh the Android editor state without redrawing
the unchanged Canvas; preview changes still redraw it.
Android also shows a new object in its normal document tile renderer while
its selected proxy commits the action. This local preview does not enter the
replica or actor history. On a successful local commit with identical painted
content and no other model changes, the existing tile pixels remain visible
without a second Canvas redraw; a mismatch, remote result, or rejection
redraws from authoritative state. The CLI and proxy still own validation,
durability, and synchronization.
For a local Android stroke, pen-up appends the final stroke state and saves
the editor undo list in one selected-proxy transaction. Remote selected proxies
keep the separate commands. This retains an atomic durable final stroke and
undo entry while avoiding a second local journal write at pen-up.
Text scales with its selection. Offline edits remain locally durable. When
offline edits conflict on reconnection, both versions must be retained, with a
conflict badge and comparison panel offering either version or both. This
supersedes ADR 0003's preference for automatic overwriting. New actions carry
the causal base version and branch visibility of each affected entity. Replay
derives the same conflict snapshots on every replica; subsequent edits on each
offline branch advance its retained snapshot. The `conflict-status` operation
makes choosing either or both versions, including undo/redo, a replicated atomic
action. Legacy operations without causal metadata retain their historical
ordering; existing histories are not rewritten.

## Implementation chunks

1. Native shape creation and text placement in Android, a shared shape catalog,
   matching CLI/proxy/server validation and rendering, atomic object actions,
   and real CLI/browser/emulator checks. Initial additions are line, triangle,
   diamond, pentagon, hexagon, and star alongside existing shapes and text.
2. Selection sessions, lock leases through proxy trees, ephemeral transforms,
   atomic transform actions, conflict preservation and comparison UI. Test
   disconnects, lease expiry, offline divergence and every mutation entry point.
3. Remaining shape families, rich text controls, brushes, erasers and fill.
4. Extended layers, pictures, templates, canvas tools, and toolbar preferences.

These were delivery boundaries, not reductions in scope. The implementation now
includes each group. New shape IDs, causal metadata and conflict status actions
require upgrading app, CLI/proxies and server together before synchronization.
The durable proxy-tree transport remains protocol 3; `/api/locks` adds transient
selection coordination and preview transport without adding drawing history.

## Implementation choices

- Reachable upstream authorities arbitrate competing selections. Disconnected
  replicas continue editing their local state; causal conflicts retain both
  branches when connectivity returns. A selection is not a promise of global
  exclusivity across a network partition.
- Leases and previews are transient coordination state. Proxies keep them in
  memory; the server clears its lease tables at startup. Heartbeats and previews
  do not write the action journal. Clean release is propagated and leases expire after
  15 seconds. A completed transform creates one normal reversible batch.
- Native geometric families, text, images and ink remain editable entities.
  Rotation and reflection share the same rendering transforms on Android,
  desktop export and the browser. Group transforms scale text with geometry.
  Box and freeform lasso selection use the same lease acquisition path.
- New partial erasing preserves item identity and applies a local coverage mask
  to ink or objects, as specified in [ADR 0011](0011-unified-draw-item-coverage.md).
  Historical fragment-based eraser actions retain their original replay behavior.
  Area and page erasing use visibility operations.
  Scribble-to-erase invokes the same erase command and is a saved preference.
- Enclosed fill supports a closed native object, a closed ink outline, or a
  bounded flood region across several native outlines. Filled regions are SVG
  objects. It does not flood-fill pixels inside imported pictures or arbitrary
  SVG artwork. Adjacent equal-width scanline runs are combined into taller SVG
  rectangles so repeated fills store and draw fewer path commands without
  changing covered pixels. The Android fill gesture avoids an unchanged
  Canvas refresh while its local command is pending; fills still pending after
  150 ms refresh the editor chrome to show progress. Templates use an editable
  background layer below drawing layers.
- Brush profiles are rendered from the stroke centerline as a pressure-varying
  swept ribbon, not as a sequence of overlapping point dabs. The shared renderer
  resamples by traveled distance, derives tangent/normal frames, and composites
  pigment coverage with additional deposits at self-crossings (ADR 0016); grain and bristle lanes are positioned by arc length so their
  appearance is independent of tablet sample density. Marker opacity therefore
  builds from actual overlap, not from dense input points. An optional per-stroke
  `material` percentage (0–100, default 50) tunes each profile's characteristic
  parameter; Android, CLI export and the browser viewer use the same path model. The
  point schema remains x/y/pressure, so physical stylus tilt is not recorded.
- Pictures embed PNG/JPEG data (2 MiB, maximum 8192 per edge and 32 megapixels)
  rather than external references. Atomic action and transport limits remain
  3 MB and 4 MB respectively.
- Icons are original Android vector/code drawings. The decompiled
  `ShapeFactory`, graphical menu configuration and tool-box configuration were
  behavioral references; no BOOX image assets were copied.

## Verification

Run builds and checks through the root Nix environment. Actual CLI commands must
exercise offline writes, history, validation, export and convergence. Browser
and Android checks must exercise the rendering and creation paths, not merely
assert that source code contains a new shape name. Physical stylus behavior and
E Ink refresh remain actual-hardware checks. Record executed results in
`docs/verification.md`.
