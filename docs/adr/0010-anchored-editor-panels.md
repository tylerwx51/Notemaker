# ADR 0010: Anchored monochrome editor panels

Status: accepted, 2026-10-14. Supersedes the permanent layer tray and toolbar
grouping in ADR 0002.

## Context

The user requested the organizational structure and opened-menu style studied
in BOOX Notes on the NoteAir6C. The existing tinted, rounded action cards and
permanent right layer tray consumed drawing space and gave every command the
same visual weight.

## Decision

Use a 56 dp white left rail for Template, Layers, Shapes, Fill, Select, Text,
Insert, Move, Pages, Agent and More. Agent uses an original code-drawn robot icon
above More and opens the existing Agent interface directly. The top bar carries Notes, the document identity
and save/sync status, Pen, Erase and direct Undo/Redo. More retains toolbar
preferences, presets, history and Agent access. Existing actions remain reachable;
this change adds no document actions or protocol changes.

Panels share a white surface, thin black border and modest corner radius, with
no elevation, dimmed backdrop or transition animation. They open beside the
rail or below top-bar tools, clamp to the usable window, and scroll if necessary.
A triangle on the originating tool marks the opened panel. A small black dot
marks the selected drawing tool or visual choice, without inverting the icon.
Tool labels remain optional and their preference persists; content descriptions
and tooltips identify icon-only choices.

The panel interior follows its purpose: pen settings use grouped controls and
a stroke sample, shapes use a six-column tablet grid (four on narrow screens),
layers use thumbnail rows with visibility and options, and commands use divided
icon-and-label rows. Color swatches display the ink color with black outlines
and a selected dot. Layer selection uses a heavier row outline and bold name.
The layer panel replaces the permanent tray and overlays the canvas without
changing canvas bounds or zoom. Existing visibility/force rules remain intact.

Use original code-drawn NoteMaker icons. BOOX screenshots and extracted assets
remain local research artifacts, not distributed app resources. Editing,
history, leases and synchronization continue through the existing local proxy.

## Verification

Build through `nix-shell --run 'apps/android/build.sh'`. The emulator UI review
checks actual toolbar controls, pen color, shape placement, layer visibility,
force visibility, history, panel touch isolation, stable canvas bounds, retained
label preferences and narrow-screen menu access. It saves review screenshots.
The existing tool-switch regression covers selection-release behavior.
Physical E Ink refresh and stylus ergonomics require hardware validation.
