# ADR 0012: Browse documents through the selected replica

Status: accepted, 2026-10-14.

## Context

Opening a saved note required remembering and entering its name. Users need to
see their files, select one to open, or create a new one while offline.

## Decision

Add a searchable, alphabetically sorted Files dialog under Notes. It lists the
selected replica's documents using the existing `document list` command. The
scope is explicitly displayed: this device or the selected editing proxy. List
loading and creation run on the existing worker queue, with refresh and visible
failure messages. Search is local and needs no polling or animation.

Default to a responsive grid with document previews, with a selectable compact
list view. Persist the mode as a device preference. Generate previews lazily on
a dedicated background worker through selected-proxy snapshots and the shared
Android renderer; cache at most 24 small bitmaps per open browser. Closing the
browser cancels queued work; refresh invalidates previews. Render page zero for
paged notes and fit visible content for infinite canvases, respecting hidden and
forced-visible layers. Preview failures leave the file selectable. Browsing and
preview generation do not create document actions or change the editor selection.

New file uses the shared `document create` command with a name and document type.
It rejects duplicate names atomically and opens the document only after durable
creation succeeds. Opening a listed file retains its saved type, uses the normal
editor switch path, and remembers the selection. No new content actions or CLI
protocols are introduced. The existing connection/name dialog remains available
for configuring proxies and opening upstream documents not yet replicated.

This is a browser for NoteMaker documents, not arbitrary Android storage files.
It does not enumerate upstream-only documents or change replication scope.

[ADR 0013](0013-document-deletion.md) extends this browser with confirmed
deletion through the shared proxy and excludes tombstoned documents from lists.

## Verification

Build the APK and run `tests/files_android.py` inside the Nix shell. The isolated
offline emulator checks creation, empty and duplicate names, search, opening,
restart persistence, retained multipage type, default grid previews and remembered
list mode. Preview instrumentation checks rendered ink and unchanged document
revision. Physical E Ink behavior and
remote proxy availability require separate validation.
