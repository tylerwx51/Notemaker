# ADR 0005: Embedded static SVG objects

Status: Accepted

## Context

Agents need to place generated SVG artwork in documents through the CLI, offline,
with the same local state, actor history, synchronization, and tablet rendering
as ordinary drawing actions. Converting artwork into hundreds of independent
actions would lose atomic editing and cannot preserve arbitrary filled paths.

## Decision

Extend the existing object snapshot with `shape: "svg"` and an embedded `svg`
source string. Geometry defines a clipped viewport. Placement is one ordinary
Store transaction; move/resize/source replacement, visibility, layers/pages, and
undo/redo reuse existing object actions. No server-side mutation shortcut, new
asset service, or agent orchestration is added. The CLI supports raw SVG files
and stdin plus structured placement, and existing JSON object/diagram commands
also accept SVG objects. Proxy clients transfer source rather than remote paths.

Shared validation accepts a bounded self-contained static SVG profile. It rejects
scripts, external resources, DTDs/entities, animation, and unsupported elements
before a transaction is persisted. Python server validation mirrors that profile.
Source remains intact; renderers normalize root dimensions to the viewport while
preserving viewBox origin and aspect-ratio behavior. Parsed Java render assets
use small bounded caches to avoid reparsing on tablet redraws.

Bundle hash-pinned JSVG 1.4.0 for Java2D export and AndroidSVG 1.4 for Android
canvas, thumbnails, and PNG export. (The Rust CLI renders SVG objects with resvg
instead of JSVG; see ADR 0026.) Dependency declarations live with their apps;
the root Nix shell fetches the immutable artifacts. SVG export embeds an isolated
SVG data image, avoiding ID/CSS collisions. The browser uses an SVG image rather
than inserting source into the page DOM.

## Consequences

Artwork remains vector-based and editable as a single object, including atomic
history. Individual SVG paths are not native diagram nodes. Source is repeated
in snapshots/history, so imports are limited to 256 KiB and existing transaction
size limits still apply. The supported profile and typography limitations are
documented in the CLI README. Full browser SVG, filters, masks, images, and use
references are outside this initial profile. Font metrics/CSS support can vary
between platforms; path outlines provide predictable typography.

SVG support requires updated clients and server; there is no protocol feature
negotiation or fallback for old clients. Existing document objects and replica
formats are preserved. No remote deployment or physical-tablet upgrade is implied.

The October 13 physical-device follow-up confirmed that an older client rejects
SVG events with `Unknown shape` and cannot advance its sync cursor past them.
An in-place APK upgrade preserves pending edits/history and restores catch-up;
see [verification](../verification.md#physical-boox-follow-up--2026-10-13).

## Matplotlib chart profile extension

Matplotlib SVG exports use local `use` references for outlined glyphs and plot
markers, and `pattern` for hatched bars. The static profile accepts these tags;
`use` targets must be existing local paths, which prevents recursive references.
The server and Java validators enforce the same constraint. Matplotlib's default
DTD and RDF metadata are removed by the chart generator before import; arbitrary
DTDs and external resources remain rejected. Raster image artists such as
`imshow` remain outside this vector profile. A CLI integration gallery covers ten
chart types, PNG rendering, offline placement, and synchronization.

## Update 2026-11-02

The Java implementation described here was ported one-to-one to Rust and
removed (ADR 0026 CLI, ADR 0029 Android app, `docs/design/rust-port.md`).
The decision stands; JVM, `app_process`, jsvg/androidsvg and `.java` details
now refer to their Rust equivalents (native CLI binary, resvg).
