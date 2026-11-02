# NoteMaker CLI

The CLI sends edits to a selected proxy owning a durable local replica. It never needs the server for drawing,
inspection, undo/redo, or export. Use one replica directory per device/server
workspace, shared by local users/agents, and separate directories across devices.
Do not put the replica on a network filesystem: locking and atomic rename depend
on local Linux/Android filesystem semantics.

## Build and run

All tools are supplied by the root Nix shell. The CLI is the Rust crate
`notemaker_cli` (see [RUST.md](RUST.md) for the Java → Rust API mapping). The
former Java sources are in git history at commit `0534499`.

```sh
nix-shell
apps/cli/build.sh --android
apps/cli/notemaker --help
```

`apps/cli/build.sh` runs `cargo build --release -p notemaker_cli`; artifacts are
`target/release/notemaker` (run through `apps/cli/notemaker`) plus the probe
binaries `stroke_svg_raster`, `coverage_parity`, `latency_probe` and
`cache_footprint_probe`. `--android` also cross-builds them with cargo ndk for
arm64-v8a and x86_64 (`target/<triple>/release/`), so the native binary runs on
ARM64 tablets and x86_64 emulators. SVG objects render with resvg; text uses
DejaVu from `FONTCONFIG_FILE` on the host and `/system/fonts` on Android
(`NOTEMAKER_FONT` overrides the regular face). No JVM is needed.

## Global options and output

Options precede the command:

```sh
apps/cli/notemaker --state /path/to/replica --document notes --actor diagram-agent inspect
```

- `--state`: defaults to `$NOTEMAKER_STATE`, or `$XDG_STATE_HOME/notemaker`, or
  `~/.local/state/notemaker`. The Android launcher supplies app-private storage.
- `--proxy-file`: selected running proxy descriptor, default `$NOTEMAKER_PROXY_FILE`.
  Required for writes and synchronization. Without it, `--state` opens a read-only
  replica; `serve` acquires exclusive ownership of that directory.
- `--document`: shared case-sensitive document name, default `notes`.
- `--actor`: stable editing session identity, default `$NOTEMAKER_ACTOR` or `cli`. Give distinct agents
  distinct IDs. The Android UI uses `human`. Calls with the same ID share history.

Successful commands return JSON on stdout (except `--help`); errors return JSON
on stderr and exit 1.
Input may be a JSON file or `-` for stdin. Do not pass JSON through a shell when
launching from an application: use argument arrays and write stdin.

Mutations return an operation ID and, where applicable, generated object/layer IDs.
Mutations are local transactions, not idempotent requests: if a caller loses the
result, inspect `log` before retrying. Synchronization retries are idempotent.
No command contacts an AI provider. Proxy commands contact the selected proxy,
which synchronizes configured documents automatically; upstream addresses and
credentials are not embedded in source.

## Delete a document

```sh
apps/cli/notemaker --proxy-file /path/to/replica/proxy.json --document unwanted-note --actor human document delete
```

Returns `{"document":"unwanted-note","deleted":true,"operation":"…"}` after
the selected proxy durably saves one deletion action. Repeating deletion returns
`deleted:true` without another action (and omits `operation`). This command is
idempotent, unlike ordinary content edits. It needs no JSON input or server
connection. The proxy synchronizes the deletion automatically when connected;
`sync [UPSTREAM]` can request a pass. Success means local persistence, not a
server acknowledgement; inspect `status.pending` to check outstanding actions.

Deletion removes the file from `document list` locally and from the server's
active document list after sync. `inspect` and `status` expose `deleted:true`.
Other replicas receive the tombstone through ordinary synchronization, including
replicas with stale pending edits. The tombstone permanently wins over edits and
creation, so the name cannot be reused. Content edits and undo/redo are rejected
once the replica knows it is deleted. Actor histories and operation logs remain
available for audit and replication; deletion does not erase stored bytes.

You can delete a known server document by name even before downloading it; an
unknown name is also permanently tombstoned. Use the exact intended name.
No direct server mutation or standalone `--state` write is involved. Upgrade
the app, CLI/proxies and server together before using document deletion.
The current trusted-LAN server has no authentication.

## Actions

| Command | Input / result |
| --- | --- |
| `document create [fixed\|pages\|infinite]` | Create a document; existing document creation fails. Default fixed. |
| `document list` | List documents in this local directory. Does not discover server documents. |
| `inspect` | Effective document type, ordered layers/strokes/objects and active-state maps. Missing active entry means true. |
| `status` | Pending count, cursor, local revision and pinned server identity. |
| `checkpoint` | Atomically fold the current local journal into the base file without changing the revision or actor histories. Requires the selected proxy; returns revision and journal byte counts. |
| `log` | Committed events, pending operations, actor histories and local settings. |
| `diagram add FILE\|-` | Create a new layer and editable objects atomically. |
| `svg add FILE.svg\|- [PLACEMENT.json]` | Place a static SVG as one object; `-` reads raw SVG from stdin. |
| `layer put FILE\|-` | Create/update `{layer,name,force}`. |
| `object put FILE\|-` | Create/replace a complete object (defaults below). |
| `object put snapshot FILE\|-` | Create/replace an object and return `{ "operation": "...", "actions": [...], "snapshot": {...} }` from the same durable edit. |
| `object update ID FILE\|-` | Patch geometry, label, style or attachments within `scope:{layer,page}` (default base/0). |
| `object hide ID [LAYER [PAGE]]` / `object show ID [LAYER [PAGE]]` | Erase/restore independently of geometry within the named scope (default base/0). |
| `layer hide ID` / `layer show ID` | Shared layer active state; base cannot be deactivated. |
| `stroke put FILE\|-` | Create/replace a protocol stroke snapshot. |
| `stroke hide ID [LAYER [PAGE]]` / `stroke show ID [LAYER [PAGE]]` | Whole-stroke erasure/restoration within the named scope (default base/0). |
| `apply FILE\|-` | Atomic array of layer/object/stroke/active operations; no document creation or nested batches. |
| `undo` / `redo` | Compensating action for the selected actor. |
| `export FILE.svg\|FILE.png [PAGE [X Y WIDTH HEIGHT]]` | Render a page/viewport; page is zero-based. Default viewport 0,0,1000,1400. |
| `sync [UPSTREAM]` | Request a rate-limited proxy synchronization pass; optional URL or proxy descriptor path configures its sole upstream. A receipt from an intermediate proxy leaves actions pending until the main server commits them. |
| `configure UPSTREAM` | Set the sole upstream URL or descriptor path (on the proxy host); empty string disconnects. |
| `snapshot` | Materialized model plus revision, pending/event counts and local editor preferences. |
| `append FILE\|-` | Append one raw protocol action with a generated ID; used for editor strokes with editor-managed undo. |
| `append snapshot FILE\|-` | Append one raw action and return `{ "operation": "...", "snapshot": {...} }` from the same durable edit; used for stroke undo/redo. |
| `append settings FILE\|-` | Atomically append `{"operation": {...}, "settings": {...}}` to the selected local proxy, returning `{ "operation": "...", "saved": true }`. The settings use the `settings` schema below and remain local. |
| `settings FILE\|-` | Local editor metadata: hidden, undo, redo string arrays, selected layer and page. Never replicated upstream. |
| `undo snapshot` / `redo snapshot` | Apply one actor-history action and return `{ "operation": "...", "snapshot": {...} }` from the same durable edit. The ordinary `undo` and `redo` outputs stay unchanged. |

Inspection retains erased entities for recovery; use the active maps and layer
membership to determine what is currently visible. Export ignores UI-local hiding
preferences, but respects shared object/stroke/layer active state. Default export
clips infinite canvases to the default viewport; request another viewport as needed.
PNG is capped at 16 million pixels, with each dimension at most 8192.
Text supports multiline content, font family, size, bold, italic, underline and
alignment, with explicit line breaks and no automatic wrapping.
Fonts/antialiasing vary across Android, desktop, and browser; geometry
and replay rules are shared, but raster bytes are not promised to match.
The Android child uses `/system/fonts/Roboto-Regular.ttf`; set `NOTEMAKER_FONT`
in its environment for another readable font/character coverage. Its isolated font
initialization uses framework APIs, verified on API 36; BOOX firmware still needs
a hardware check. Ordinary app rendering retains the system's full font fallback.

## Place a generated SVG

These examples require a running proxy selected by `NOTEMAKER_PROXY_FILE`; see
[First diagram](#first-diagram) for startup.

Run inside `nix-shell` after building the CLI:

```sh
cat > /tmp/placement.json <<'JSON'
{"object":"generated-figure","x":100,"y":200,"w":600,"h":300,"layer":"base","page":0}
JSON
apps/cli/notemaker --document demo --actor agent-1 svg add generated.svg /tmp/placement.json
apps/cli/notemaker --document demo --actor agent-1 object update generated-figure - <<'JSON'
{"x":150,"y":250,"w":400,"h":200}
JSON
apps/cli/notemaker --document demo export /tmp/demo.png
```

Placement JSON is optional. Defaults are `x=0`, `y=0`, `layer="base"`, `page=0`,
a generated object ID, and `w`/`h` from the SVG's viewBox dimensions (or its
pixel width/height if no viewBox is present). Supply both `w` and `h` for a custom
viewport. The SVG's `preserveAspectRatio` controls its fit (default centered meet).
Layers must already exist; pages above zero require a `pages` document. Placement
coordinates are document units. A supplied object ID must be new.

The original SVG is embedded in the local replica, so deleting the source file
does not affect the document. Placement, resizing, replacement, hide/show, and
actor undo/redo work offline and synchronize normally. The SVG is one object;
its internal paths are not individual editable diagram nodes. To replace its
contents, use `object update ID` with `{"svg":"<svg ...>...</svg>"}`. JSON-only
callers can also use `object put` with `{"shape":"svg","svg":"...",...}`;
SVG objects work in atomic `diagram add` and `apply` actions.

The local proxy reads files on the calling machine and transfers their contents;
the tablet does not need access to those paths. A direct proxy request uses
`args:["svg","add","-"]` and `input:{"svg":"...","x":100,...}`.
SVG export embeds an isolated SVG image; PNG, Android canvas/layer previews, and
the browser render the vector asset in its viewport.

For a Matplotlib gallery, run `python3 apps/cli/examples/matplotlib_charts.py
/tmp/notemaker-charts` inside `nix-shell`, then place a file with `svg add` as
above. The generator uses path outlines for labels and removes Matplotlib's XML
DTD and RDF metadata wrapper. Do the same normalization when generating your own
figures; raw `savefig(..., format="svg")` output contains a DTD that NoteMaker
rejects. Use vector artists such as `pcolormesh` for heatmaps; `imshow` emits an
embedded raster image outside the supported profile.

Supported imports are static, self-contained SVGs up to 256 KiB with an SVG
namespace and positive viewBox or pixel dimensions. Paths (including curves),
basic shapes, groups/transforms, text/tspans, gradients, clipping, markers, and
basic CSS are supported. Local `use` references to paths and static patterns
support Matplotlib glyphs, markers, and hatches. Scripts, animation, external
resources, XML DTDs, `foreignObject`, embedded images, filters, and masks are rejected.
CSS imports and escapes are rejected; resource references must be local `#id`
fragments. Unsupported elements/attributes fail before an edit is committed.
Fonts and some CSS/text details differ across renderers; convert text to paths
when exact typography matters. Upgrade all clients and the server before syncing
SVG objects; older versions do not understand the new shape.

## First diagram

Start `notemaker --state /path/to/replica --document demo serve 8766` in a
separate Nix shell. In the client shell, set
`export NOTEMAKER_PROXY_FILE=/path/to/replica/proxy.json` before the editing
examples in this document. Stop the proxy with Ctrl-C. Reads can instead use
`env -u NOTEMAKER_PROXY_FILE notemaker --state /path/to/replica inspect`.


```sh
apps/cli/notemaker --document demo --actor agent-1 diagram add apps/cli/examples/diagram.json
apps/cli/notemaker --document demo --actor agent-1 object update app - <<'JSON'
{"scope":{"layer":"architecture","page":0},"x":150,"text":"Renamed app"}
JSON
apps/cli/notemaker --document demo export /tmp/demo.png
apps/cli/notemaker --document demo --actor agent-1 undo
apps/cli/notemaker --document demo --actor agent-1 undo
apps/cli/notemaker --document demo --actor agent-1 redo
```

The first undo reverts the label/move; the second hides the whole diagram and its
layer; redo restores it. Repeating the example's fixed layer/object IDs fails to
avoid overwriting an existing diagram. Omit IDs to generate UUIDs, or assign new
IDs when creating another diagram.

Diagram JSON is `{name, layer?, objects:[...]}`. The layer is new; all objects in
that action are assigned to it. Connector `from` and `to` IDs refer to non-arrow
nodes in that diagram on the same page. Connectors follow node boundaries when
nodes move or resize. Erasing a node does not automatically erase its arrows.

Object fields:

| Field | Meaning / default |
| --- | --- |
| `object` | Stable ID, generated UUID if omitted on creation. |
| `shape` | Required: `rect`, `ellipse`, `text`, `arrow`, `line`, `triangle`, `diamond`, `pentagon`, `hexagon`, `star`, or `svg`. |
| `svg` | Required for SVG objects: embedded static SVG source. See the import profile above. |
| `layer`, `page` | `base`, `0` (diagram chooses its new layer). |
| `x`, `y`, `w`, `h` | Logical coordinates; defaults `0,0,160,80` (SVG size comes from source). Dimensions must be positive except for lines and arrows, whose endpoint is `(x+w,y+h)`. |
| `color`, `width` | Signed ARGB color, default -16777216 (black); width 1–100, default 3. |
| `fill` | Optional signed ARGB fill; omitted means transparent. |
| `text`, `fontSize` | Plain label, default empty; font size 1–500, default 24. |
| `from`, `to` | Optional arrow attachment IDs. Unattached end uses `(x,y)` or `(x+w,y+h)`. |

SVG styling and text come from the embedded source; the object's generic
`color`, `width`, `fill`, `text`, and `fontSize` do not override it.

`shape list` returns `{"shapes":[{"id":"rect","name":"Rectangle"},...]}`.
This is a read-only catalog available directly or through a selected proxy;
it does not add document actions or history. For example, save the following
JSON as `triangle.json`, then run the command inside `nix-shell`:

```json
{"object":"triangle-1","shape":"triangle","x":100,"y":100,"w":160,"h":120,"color":-16777216,"width":4,"fill":-3355444}
```

```sh
apps/cli/notemaker --proxy-file .test-artifacts/my-replica/proxy.json --document demo object put triangle.json
```

Native polygons preserve their shape ID and editable bounds; shared rendering
derives their geometry. Fill is ignored for lines and arrows. Upgrade all
clients, proxies and the main server before synchronizing these shape IDs.

Pages are numbered 0–10000 in the protocol/CLI; the app shows page numbers starting
at 1. Fixed and infinite documents display page 0. Coordinates are document units,
not device pixels. Geometry must be finite and within ±1,000,000 units. Inputs
are limited to 3 MB. Atomic actions have at most 200 primitive operations including
visibility records (a new layer plus 99 new objects is the maximum new diagram).

## Android embedding

### Live app sessions

Each Activity launch gets a new session UUID. The ID survives background and
foreground transitions and expires when that Activity is destroyed. A running
tablet publishes its live session through its configured proxy tree, so a CLI
connected to any proxy under the same main server can discover and control it:

```sh
apps/cli/notemaker --proxy-file selected-proxy.json ui sessions
apps/cli/notemaker --proxy-file selected-proxy.json ui actions SESSION
apps/cli/notemaker --proxy-file selected-proxy.json ui state SESSION
printf '{"name":"abc"}' | apps/cli/notemaker --proxy-file selected-proxy.json ui action SESSION document.open -
printf '{"page":2}' | apps/cli/notemaker --proxy-file selected-proxy.json ui action SESSION page.open -
```

`ui actions` returns the registry of semantic action names and JSON argument
shapes. `ui action` returns its name and screen state immediately after dispatch;
content operations that use the replica worker may finish later. Files controls,
document/page/layer navigation, tool and brush choices, eraser options, canvas
view changes, selection controls, templates, history buttons, and Agent panel
open/close are available. Use the existing document CLI commands for content
creation and edits. The Files browser currently lists documents without folders.

GIF controls are `gif.import.open` for Android's document picker,
`gif.player.open`, `gif.select` with `{id}`, `gif.play`,
`gif.pause`, `gif.next`, `gif.previous`, and `gif.seek` with a zero-based `{frame}`.
GIFs also show these controls directly beneath the item. Select the toolbar's
Interact tool with `tool.select` and `{"name":"interact"}` to use them by touch.
Every action returns `state.gifPlayer`; `ui state` also reports the selected GIF
and each loaded clip's frame, count, duration, and play state. Player actions
affect only the addressed Activity session and do not create document history.
Animations start paused. Android uses each GIF's frame delays with a 100 ms
minimum to limit E Ink redraws; leaving the app pauses playback.

Each session record includes `deviceId`, `deviceName`, `document`, `page`,
`layer`, `tool`, `foreground`, `startedAt`, `proxyId`, `lastSeen`, and `expires`.
The root drops a session after 12 seconds without a heartbeat. When an upstream
is unreachable, `ui sessions` reports only sessions under the reachable proxy.
Screen commands require a running Activity and are never journaled or replayed
after it closes. A timeout means the result is uncertain; inspect `ui state`
before retrying an action. Keep proxy bearer tokens private. The main server's
UI endpoints follow its existing trusted-network security model.

Run `nix-shell --run 'python3 tests/ui_sessions_android.py'` for the live API 36
session check, then `nix-shell --run 'python3 tests/ui_topology_android.py'` for
the two-proxy, main-server route.

Call `MainActivity::start_cli(&[&str])` (Rust, `apps/android/src/main_activity.rs`)
off the UI thread, write input, drain stdout **and** stderr concurrently, wait for
completion with a timeout, and check the exit code. It returns a
`std::process::Child` with all three streams piped; orchestration and process
lifecycle management belong to the caller. Example arguments:

```rust
app.start_cli(&["--actor", "agent-session-17", "diagram", "add", "-"])
```

The launcher runs the APK's native CLI (`<nativeLibraryDir>/libnotemaker_cli.so`,
the `notemaker` binary packaged under a library name so Android extracts it) and supplies the current document and app's
private replica proxy descriptor. The child uses the app's authenticated
selected proxy (the device's localhost proxy by default). No root, writable executable extraction, or Termux is required.
Android may stop the app and its children; durable pending edits survive this and
synchronize when the app next runs. The local service synchronizes while Android
permits it to run. Force-stop ends the service. Desktop users run `serve` with an
upstream URL or proxy descriptor for continuous synchronization.

`tests/cli_android.py` invokes this launcher through instrumentation in the actual
Activity; it does not substitute `adb shell` for the app-spawned CLI process.

## Conflict and history semantics

Only the proxy writes document state. It holds the replica-directory ownership
lock and serializes durable document transactions. Direct readers take the
document lock and see complete snapshots; they do not repair journal tails.
The proxy recovers incomplete tails. Its single sync worker never holds the
document lock during network I/O. Pending actions survive intermediate receipts
and are removed only as their main-server commits are durably applied.

The server establishes a total order. Geometry snapshots use last committed
operation wins; erasure uses its own active-state register. Offline optimistic
edits can visibly change after synchronization as the final server order becomes
known. Independent objects/strokes merge. No timestamp or clock synchronization
is needed. Documents keep their first committed creation type.

Undo is actor-scoped compensating history, not selective collaborative undo:
undoing an edit to an existing object restores that actor's earlier snapshot and
may supersede another actor's later geometry edits to that object. Undoing diagram
creation hides its entire layer, including any subsequent content placed there.
These are explicit shared actions; the audit log keeps the displaced snapshots.
Prefer a new agent-owned layer for independent contributions.

## Local proxy

Use `notemaker --state DIR --document NAME serve [PORT [UPSTREAM]]` to start
the owner (default port 8766). UPSTREAM is a main-server URL or private descriptor
path for another durable proxy; `https://` URLs are verified against the
platform CA store, and `NOTEMAKER_EXTRA_CA_FILE=ca.pem` adds trusted CAs (for
example a self-signed home-server CA). No upstream is needed for editing. One proxy
owns a directory; a second owner fails. `--proxy-file FILE` or
`NOTEMAKER_PROXY_FILE` selects it for clients. Standalone mutations and the old
`watch` command are no longer supported.

The private descriptor is `{"url":"http://127.0.0.1:8766","token":"…","id":"…"}`.
Identity and token survive restart in `proxy-config.json`, which also persists
the upstream and pinned main-server identity. Back up this file with the replica.
The listener binds to loopback; use ADB/SSH forwarding for another device.
New proxy links validate the ancestor chain and reject cycles (maximum 64 hops).
Proxy ancestors must be reachable to validate a new link; an existing link can
remain configured while disconnected. Each proxy has one upstream for all its
locally opened documents.

Commands use `POST /command`, `Authorization: Bearer <token>` and JSON:

```json
{"document":"notes","actor":"agent-1","args":["object","update","app","-"],"input":{"scope":{"layer":"architecture","page":0},"x":150,"text":"Updated"}}
```

Responses are `{"result": ...}` or HTTP 400 `{"error": ...}`. Successful mutations
follow journal fsync. Each diagram/batch is atomic and actor histories remain
separate. All action/read commands above except `serve` and `export` are accepted.
CLI export fetches the effective model and writes the image in the client process.
Input files are read by the client; the proxy receives their contents in `input`.
Request bodies are capped at 4 MB.

The first pending action starts a 500 ms timer that later actions do not reset.
One upload is in flight per proxy, request starts are at least 500 ms apart,
and each batch has at most 100 actions and approximately 3 MB of action JSON.
Failures retry with exponential backoff capped at 30 seconds. SSE
`GET /api/stream` notifications wake catch-up; 15-second heartbeats do not poll
document state. Durable cursors recover all changes after reconnection.
See [replication protocol](../server/PROTOCOL.md#version-3-durable-proxy-trees).

Direct `--state` readers can read while the proxy runs or after it stops. They
reconstruct their model once per process and observe complete journal records.
Preserve each baseline and journal together for backup under the document lock
or with writers stopped. Never run older standalone writers against this state.

## Drawing tools and collaboration

Use the same `--proxy-file`, `--document` and `--actor` for each editing session.
All commands below return JSON. A failed validation or locked dependency leaves
the entire content action and its undo history unchanged.

| Command | Input or behavior |
| --- | --- |
| `shape list` | Shape IDs/names, including geometric families and cube, cylinder, cone, sphere and pyramid. |
| `object put -` / `object update ID -` | Native shape/text snapshots or patches. Text supports `bold`, `italic`, `underline`, `fontFamily` (`sans-serif`, `serif`, `monospace`), and `alignment` (`left`, `center`, `right`). |
| `stroke put -` | Existing stroke schema plus `brush`: `pen`, `pencil`, `marker`, `brush`, `oily`, `fountain`, `charcoal`, `neoBrush`, `latinCalligraphy`, `asiaCalligraphy`, `square`; optional `material` is an integer from 0–100 (default 50) controlling that profile's grain, opacity, load, softness, flex or nib angle. |
| `erase -` | `{mode, page?, layer?, points?, radius?}`. Modes: `item` (`stroke` is a compatibility alias), `partial`, `area`, `page`. Whole-item and area modes test painted coverage against the swept round eraser, not its bounding rectangle. Page mode omits points. Partial erasing adds a local mask to any item type, preserving its ID/source in one undoable action. Defaults: layer `base`, page `0`, radius `12`. |
| `fill -` | `{object, color, layer?, page?}` for a native closed shape, `{stroke, color, layer?, page?}` for a closed ink outline, or `{x,y,page,layer,color}` for a region enclosed by several native ink/shape boundaries. Defaults to `base`/`0`. Open regions, including erased boundary gaps, are rejected. |
| `image add FILE [PLACEMENT.json]` | Embedded PNG/JPEG; maximum 2 MiB decoded, 8192 pixels per edge, 32 megapixels. Placement uses the SVG placement fields. No external image URLs. |
| `gif add FILE [PLACEMENT.json]` | Embedded GIF with at most 500 frames; maximum 2 MiB decoded, 8192 pixels per edge, 32 megapixels. Placement uses the image placement fields. Playback state belongs to each live app session. |
| `template add -` | `{template,page?,x?,y?,w?,h?,spacing?,color?,background?}`. Templates: `blank`, `ruled`, `grid`, `dots`, `planner`. Defaults to a reusable page background layer below drawing layers; `background:false` inserts into the supplied layer. |
| `layer duplicate ID` | Copy layer and active contents atomically, remapping internal connector references. |
| `layer merge SOURCE TARGET` | Move source contents into target and archive source; base cannot be the source. |
| `layer reorder ID INDEX` | Zero-based drawing order. |
| `layer hide ID` / `layer show ID` | Reversible archive/restore; the base layer remains available. |
| `selection query -` | `{layer?,page?,region?:[[x,y],...],topmost?:boolean}`. Returns `{items:[{kind,id,page}]}` without acquiring leases or writing history. Region is an even/odd polygon (box or lasso); omit it to list items with coverage. `topmost:true` returns the first painted hit. Defaults to `base`/`0`. |
| `selection select -` | `{session?,layer?,page?,entities:[{kind:"object" or "stroke",id}]}`. Resolve every ID within one layer/page before acquiring leases; mixed-layer batches fail without side effects. Defaults to `base`/`0`. Returns `session`, `selected`, `skipped` and lease expiry. Locked entities are skipped explicitly. A session cannot change layers/pages. |
| `selection heartbeat SESSION` | Renew the 15-second selection lease. |
| `selection deselect SESSION` / `selection disconnect` | Release one session / all selections for this actor. |
| `selection duplicate SESSION` / `selection delete SESSION` | Atomic operations on the leased selection. |
| `selection copy SESSION` / `selection cut SESSION` | Copy snapshots, or return snapshots and delete the selection in one action. Internal connector references are retained; external attachments are materialized. |
| `selection paste -` | `{entities:[snapshots],layer?,page?,dx?,dy?}`. Paste up to 100 entities with new IDs and remapped connectors in one action. The local `settings.clipboard` stores the payload for reuse. |
| `transform preview -` / `transform commit -` | `{session,entities:[{kind,id,page}],transform:{dx?,dy?,scale?,rotation?,flipX?,flipY?,pivotX?,pivotY?}}`. Rotation is in degrees; scale is positive. Default pivot is the selection center. Text scales with the selection. |
| `transform cancel SESSION` | Discard the transient preview without changing content. |
| `conflicts` | Both snapshots, visibility, actor names and version IDs for unresolved concurrent edits. |
| `conflict resolve ID local\|remote\|both` | Choose Version A, Version B or retain both; resolution is atomic and undoable. These names describe replay branches, not necessarily the calling device. |

Renew a selection before it expires. Preview updates are transient and do not
create action history or journal entries. Commit the completed gesture once;
cancellation, lost leases and expiry discard the preview. Undo and bulk edits
that affect another session's locked content are blocked as a whole.

Targeted updates use the same scope. For example, `object update app -` accepts
`{"scope":{"layer":"architecture","page":0},"x":150}`. Without `scope`, it edits
only `base`/`0`. `object hide ID [LAYER [PAGE]]` and `object show ID [LAYER [PAGE]]`
(also `stroke hide/show`) use those same defaults. Creation/replacement snapshots
carry their own layer/page; replacing an existing ID cannot move it out of its
own scope. Use layer management or cut/paste to move between layers. Raw
`apply`/`append`, replication and actor-history replay remain document operations,
independent of an interactive selection session.

Coverage lives in item-local coordinates. Ink, basic shapes and text use native
vector regions. Images, imported SVG and SVG-backed figures use lazy alpha tiles
at one local document unit per pixel: 64 × 64 pixels, empty/full sentinels, and
512 bytes for a mixed tile. Each cached item retains at most 128 tiles; the LRU
retains 16 items (at most 1 MiB of mixed-tile bit payload, excluding geometry,
assets and map overhead). Large raster queries exceeding 16,384 tiles fail
explicitly; query a smaller region. Font/SVG edge details follow the platform
renderer. White opaque pixels count as coverage; transparent holes do not.

Partial erasure stores `erasures`, an array of local polygon contours. Object
coordinates are normalized by width/height (zero dimensions use one); stroke
coordinates are offsets from the first point. Stroke transforms also transform
these offsets. At most 2,000 contours and 64,000 total vertices are accepted,
within existing action-size limits. `object update` with `erasures:[]` resets a
mask. Fully erased items retain their source for undo or explicit editing.
Upgrade app, CLI/proxies and server/viewer together before syncing masked items;
older clients do not render the new masks. Existing action history is unchanged.

Fill produces an editable SVG object. Tap fill rasterizes native stroke and
geometric boundaries on a bounded grid before generating the SVG region; it
does not perform bitmap flood-fill inside imported pictures or arbitrary SVGs.
Brush profiles are original renderings rather than copied BOOX assets.
