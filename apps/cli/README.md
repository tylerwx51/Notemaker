# NoteMaker CLI

The CLI sends edits to a selected proxy owning a durable local replica. It never needs the server for drawing,
inspection, undo/redo, or export. Use one replica directory per device/server
workspace, shared by local users/agents, and separate directories across devices.
Do not put the replica on a network filesystem: locking and atomic rename depend
on local Linux/Android filesystem semantics.

## Build and run

All tools are supplied by the root Nix shell. SVG rendering libraries are pinned
by URL and hash in `apps/cli/dependencies.nix` (JSVG, JVM) and
`apps/android/dependencies.nix` (AndroidSVG). Nix fetches them at shell setup;
the builds bundle them, so installed applications need no runtime downloads.

```sh
nix-shell
apps/cli/build.sh --android
apps/cli/notemaker --help
```

Artifacts: `apps/cli/build/notemaker.jar` (JVM bytecode, JDK 17 in the Nix shell)
and `apps/cli/build/notemaker-android.jar` (DEX, API 26+). Android uses the device's
ART runtime, so the DEX artifact works with either ARM64 tablets or x86_64 emulators.
The NixOS launcher needs Java; this is not a standalone native executable.
`apps/android/build.sh` compiles the same CLI/core into the APK.

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
The current trusted-LAN server has no authentication.

## Actions

| Command | Input / result |
| --- | --- |
| `document create [fixed\|pages\|infinite]` | Create a document; existing document creation fails. Default fixed. |
| `document list` | List documents in this local directory. Does not discover server documents. |
| `inspect` | Effective document type, ordered layers/strokes/objects and active-state maps. Missing active entry means true. |
| `status` | Pending count, cursor, local revision and pinned server identity. |
| `log` | Committed events, pending operations, actor histories and local settings. |
| `diagram add FILE\|-` | Create a new layer and editable objects atomically. |
| `svg add FILE.svg\|- [PLACEMENT.json]` | Place a static SVG as one object; `-` reads raw SVG from stdin. |
| `layer put FILE\|-` | Create/update `{layer,name,force}`. |
| `object put FILE\|-` | Create/replace a complete object (defaults below). |
| `object update ID FILE\|-` | Patch existing object's geometry, label, style, layer/page or attachments. |
| `object hide ID` / `object show ID` | Erase/restore independently of geometry. |
| `layer hide ID` / `layer show ID` | Shared layer active state; base cannot be deactivated. |
| `stroke put FILE\|-` | Create/replace a protocol stroke snapshot. |
| `stroke hide ID` / `stroke show ID` | Whole-stroke erasure/restoration. |
| `apply FILE\|-` | Atomic array of layer/object/stroke/active operations; no document creation or nested batches. |
| `undo` / `redo` | Compensating action for the selected actor. |
| `export FILE.svg\|FILE.png [PAGE [X Y WIDTH HEIGHT]]` | Render a page/viewport; page is zero-based. Default viewport 0,0,1000,1400. |
| `sync [UPSTREAM]` | Request a rate-limited proxy synchronization pass; optional URL or proxy descriptor path configures its sole upstream. A receipt from an intermediate proxy leaves actions pending until the main server commits them. |
| `configure UPSTREAM` | Set the sole upstream URL or descriptor path (on the proxy host); empty string disconnects. |
| `snapshot` | Materialized model plus revision, pending/event counts and local editor preferences. |
| `append FILE\|-` | Append one raw protocol action with a generated ID; used for editor strokes with editor-managed undo. |
| `settings FILE\|-` | Local editor metadata: hidden, undo, redo string arrays, selected layer and page. Never replicated upstream. |

Inspection retains erased entities for recovery; use the active maps and layer
membership to determine what is currently visible. Export ignores UI-local hiding
preferences, but respects shared object/stroke/layer active state. Default export
clips infinite canvases to the default viewport; request another viewport as needed.
PNG is capped at 16 million pixels, with each dimension at most 8192.
Text is plain multiline sans-serif, with explicit line breaks and no automatic
wrapping. Fonts/antialiasing vary across Android, desktop, and browser; geometry
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

Supported imports are static, self-contained SVGs up to 256 KiB with an SVG
namespace and positive viewBox or pixel dimensions. Paths (including curves),
basic shapes, groups/transforms, text/tspans, gradients, clipping, markers, and
basic CSS are supported. Scripts, animation, external resources, XML DTDs,
`foreignObject`, embedded images, `use`, filters, and masks are rejected.
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
{"x":150,"text":"Renamed app"}
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
| `shape` | Required: `rect`, `ellipse`, `text`, `arrow`, or `svg`. |
| `svg` | Required for SVG objects: embedded static SVG source. See the import profile above. |
| `layer`, `page` | `base`, `0` (diagram chooses its new layer). |
| `x`, `y`, `w`, `h` | Logical coordinates; defaults `0,0,160,80` (SVG size comes from source). All non-arrow dimensions must be positive. |
| `color`, `width` | Signed ARGB color, default -16777216 (black); width 1–100, default 3. |
| `fill` | Optional signed ARGB fill; omitted means transparent. |
| `text`, `fontSize` | Plain label, default empty; font size 1–500, default 24. |
| `from`, `to` | Optional arrow attachment IDs. Unattached end uses `(x,y)` or `(x+w,y+h)`. |

SVG styling and text come from the embedded source; the object's generic
`color`, `width`, `fill`, `text`, and `fontSize` do not override it.

Pages are numbered 0–10000 in the protocol/CLI; the app shows page numbers starting
at 1. Fixed and infinite documents display page 0. Coordinates are document units,
not device pixels. Geometry must be finite and within ±1,000,000 units. Inputs
are limited to 3 MB. Atomic actions have at most 200 primitive operations including
visibility records (a new layer plus 99 new objects is the maximum new diagram).

## Android embedding

Call `MainActivity.startCli(String... args)` off the UI thread, write input, drain
stdout **and** stderr concurrently, wait for completion with a timeout, and check
the exit code. The method returns a standard `java.lang.Process`; orchestration
and process lifecycle management belong to the caller. Example argument array:

```java
new String[]{"--actor", "agent-session-17", "diagram", "add", "-"}
```

The launcher runs `/system/bin/app_process /system/bin com.notemaker.cli.Main`,
sets `CLASSPATH` to the installed APK, and supplies the current document and app's
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
path for another durable proxy. No upstream is needed for editing. One proxy
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
{"document":"notes","actor":"agent-1","args":["object","update","app","-"],"input":{"x":150,"text":"Updated"}}
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
