# NoteMaker

## Version control

This repository uses Jujutsu with a colocated Git repository. Git, Jujutsu, and
SSH are provided by `shell.nix`.

```sh
nix-shell
jj status
jj git fetch
jj git push --bookmark main
```

The `origin` remote is `tyler@192.168.50.129:/git-repos/NoteMaker.git` on the
Phenom II home server. Its address is assigned automatically; consult
`../computer-it/NETWORK_OPERATING_GUIDE.md` if it changes. To create another
local checkout:

```sh
jj git clone tyler@192.168.50.129:/git-repos/NoteMaker.git NoteMaker
```

Move the `main` bookmark to the commit you intend to publish before pushing.

## Android testing

Use a recent Nixpkgs channel with Android API 36 support. `shell.nix` enables
the Android SDK license and unfree packages for this shell.

```sh
nix-shell
boox-emulator
```

The launcher creates a persistent AVD on first use under `$HOME/.android/avd`
(or `$ANDROID_AVD_HOME`). It uses Android 16 / API 36 with Google APIs and an
x86_64 image for accelerated testing on an x86_64 Linux host. The portrait
1860 × 2480 display at 300 dpi approximates the BOOX Note Air6 C's 10.3-inch
screen. Its Android logical density may differ; confirm layouts on real hardware.
The emulator does not reproduce BOOX firmware, E Ink refresh, color rendering,
or stylus latency and pressure.

For automated testing, launch in a separate terminal or background process:

```sh
boox-emulator-headless &
adb wait-for-device
until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do
  sleep 2
done
adb install -r path/to/app.apk
adb shell screencap -p /sdcard/screenshot.png
adb pull /sdcard/screenshot.png
adb emu kill
```

Emulator flags can be passed through, for example `boox-emulator -wipe-data`.
Linux acceleration requires access to `/dev/kvm` (usually membership in the
`kvm` group). Use `emulator -accel-check` to check host support. For GUI use,
a working graphical session is required; headless mode supports CLI testing.
The initial `nix-shell` downloads a large SDK and system image.

References: [Nixpkgs Android tooling](https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/android.section.md),
[BOOX Note Air6 C specifications](https://shop.boox.com/products/noteair6c).

## Applications

The repository has three applications and a shared Java core:

- `apps/android`: native Android 16 note-taking app for BOOX Note Air6 C.
- `apps/server`: local HTTP server, durable SQLite operation log, and live browser viewer.
- `apps/cli`: agent-facing CLI compiled for NixOS/JVM and Android.
- `packages/core`: shared local replica, atomic actions, replay, and rendering.

Build and run everything inside the Nix environment:

```sh
nix-shell
apps/android/build.sh
python3 apps/server/server.py --database notemaker.sqlite3
```

The server listens on all interfaces at port 8765 by default. Open
`http://localhost:8765` on this computer to view a document. On the tablet, open
**Notes → Open or create document**, enter `http://<this-computer-LAN-address>:8765` and a document name,
and choose a type when creating a new document. Allow inbound TCP 8765 in the
computer's firewall if needed. For the emulator, use `http://10.0.2.2:8765`.
The server URL can be left blank for local-only editing.
Server flags include `--host`, `--port`, and `--database`; no remote deployment is
performed. No authentication is enabled; this version is for a trusted network.

Install and launch the APK:

```sh
boox-emulator
# In another nix-shell terminal, or use your USB-connected tablet:
adb install -r apps/android/build/notemaker.apk
adb shell am start -n com.notemaker/.MainActivity
```

If more than one device is attached, add `-s <serial>` to each adb command.
The APK uses an ignored development signing key in `apps/android/build`.
Keep that key to install future builds over an existing installation. Uninstalling
the app or clearing its data removes locally stored notes and unsynced edits.

### Drawing and collaboration

The left sidebar groups common actions into popups. Both finger and stylus draw.
**Pen** opens color swatches and width controls; stylus pressure varies width.
**Erase** removes whole strokes on the selected layer. **History → Undo/Redo**
changes your own drawing/erasing actions and syncs the result. **Move** enables
dragging/pinch zoom and offers **Reset view** and **Return to pen**.

The right layer tray shows a diagram of the layers, topmost first, with previews,
names, and visible/hidden states. Tap a card to select it or its eye to hide/show
it locally. Use **New layer** to add a named layer. Each card's **…** menu offers
**Force visible for everyone**, shown as a lock while enabled. Release it to
restore each client's local visibility preference.

The tray's top chevron collapses it to previews and eye controls; the opposite
chevron expands it. This preference survives restart. The tray overlays the page,
so toggling it preserves the canvas position and zoom. Collapse it to expose more
of the drawing surface. Show a layer before drawing on it.

Documents can be a fixed page, multiple numbered pages, or an infinite canvas.
Use **Pages** for previous/next/go-to actions in multipage documents. The document
name is its shared identity; clients opening the same name on the same server
receive the same strokes. An existing document retains its original type.
The browser viewer supports page selection, local visibility, and drag/scroll
navigation for infinite canvases; it is read-only.

Drawing continues offline. The app and agents share a local replica proxy with
an incremental model and durable journal. Pen input appears immediately;
**Saving on device…** means the worker has not finished persisting it. The proxy
retries synchronization while Android permits it to run and resumes when reopened. The server stores all committed
operations in SQLite, including intermediate stroke snapshots. Back up the
SQLite database with SQLite's backup facilities or while the server is stopped.
A document's saved sync cursor belongs to that server's database. The replica
pins its database identity on synchronization and rejects an unrelated database.
URL changes may reach the same server at a new address. Existing replicas acquire
this identity on their first sync after upgrade; point them at their original database.

### Integration verification

```sh
nix-shell --run 'apps/android/build.sh'
nix-shell --run 'python3 tests/server_checks.py'
nix-shell --run 'python3 tests/integration.py'
```

The test starts the real server on port 18765 and two isolated headless Android 16
emulators on ports 5580/5582. It does not modify the normal emulator's data.
It installs the APK, drives pressure-bearing input through the real Android
canvas, verifies identical event logs and PNG images, and checks concurrent edits,
layers, undo/redo, eraser, offline recovery, persistence, and document types.
It also checks live partial strokes during a system-injected finger gesture and
uses headless Chromium to verify the browser's replay and rendered coverage.
It stops its server and emulators on completion. First boot can take several
minutes; both emulators need KVM and may allocate about 6 GiB guest RAM combined,
plus host overhead (the API 36 emulator raises smaller requested allocations).
Artifacts are written to ignored `.test-artifacts/`.

Architecture decisions are in [docs/adr](docs/adr). Verification results and
hardware limitations are in [docs/verification.md](docs/verification.md).

For a focused UI review, start an emulator with `boox-emulator-headless`, then run
`nix-shell --run 'python3 tests/ui_review.py emulator-5554'` with its actual serial.
This installs the built APK and resets only the reserved `Interface-review` test
document. It exercises the sidebar popups and layer tray through Android's UI
automation and saves screenshots in `.test-artifacts/ui-review/`.

### Agent CLI and editable diagrams

```sh
nix-shell
apps/cli/build.sh --android
apps/cli/notemaker --help
apps/cli/notemaker --state .test-artifacts/my-replica --document demo --actor agent-1 diagram add apps/cli/examples/diagram.json
apps/cli/notemaker --state .test-artifacts/my-replica --document demo --actor agent-1 svg add apps/cli/examples/placed.svg
apps/cli/notemaker --state .test-artifacts/my-replica --document demo inspect
apps/cli/notemaker --state .test-artifacts/my-replica --document demo export .test-artifacts/demo.png
apps/cli/notemaker --state .test-artifacts/my-replica --document demo sync http://localhost:8765
```

The CLI reads and changes local state, not the server directly. `sync` exchanges
pending operations once; `watch URL` retries until stopped. A desktop does not
need to run a server to use its CLI. Separate devices use separate replicas.
Read `apps/cli/notemaker --help` for options, defaults, input/output conventions,
and quick examples; use [CLI documentation](apps/cli/README.md) for JSON schemas.
The Nix shell includes PyYAML for validating Codex skill metadata with the
skill-creator validator.
Multiple local CLI processes can share one replica and use distinct `--actor`
values. Give one agent session a stable actor ID across calls.

Diagrams contain editable rectangles, ellipses, labels, and attached arrows.
On Android use **Notes → Edit diagram objects…** to edit geometry/labels or
erase/restore an object. The app and browser render the same object model.
A diagram is one atomic, undoable action for its actor. Concurrent geometry
changes resolve in server commit order; erasure is independent of geometry.

Generated SVGs can be placed with `svg add FILE.svg [PLACEMENT.json]`. Optional
placement JSON specifies `x`, `y`, `w`, `h`, `layer`, `page`, and `object` ID.
Each SVG remains a vector asset embedded in one movable, resizable, undoable
object, available offline on both CLI runtimes. See the [SVG import profile and
examples](apps/cli/README.md#place-a-generated-svg). Nix fetches the hash-pinned
JSVG and AndroidSVG libraries, which are bundled into the CLI/APK. Rebuild and
upgrade the app, CLI, and server together before synchronizing SVG objects.

The installed Android APK includes the CLI. `MainActivity.startCli(args)` launches
it as a child process under the app's UID and supplies its private state directory
and current document. Server agent chats use the same local-state CLI workflow. The standalone Android
DEX jar is also built for diagnostics. See [CLI usage and schema](apps/cli/README.md)
and [ADR 0003](docs/adr/0003-local-replicas-and-cli.md).

```sh
nix-shell --run 'apps/cli/build.sh --android && apps/android/build.sh'
nix-shell --run 'python3 tests/cli_checks.py'
nix-shell --run 'python3 tests/cli_android.py'
```

The Android CLI check boots one isolated API 36 emulator on port 5584, launches
CLI processes from the actual app, draws concurrently while offline, checks
actor undo/redo, PNG export, desktop/tablet reconnect and conflicts, and browser
rendering. It uses a temporary server on port 18766 and writes artifacts under
`.test-artifacts/cli-android/`. It does not use or deploy to Phenom II.

### Local proxy and primary host

The current primary host is this development computer's existing server and
`notemaker.sqlite3` database. The BOOX currently connects over the USB tunnel
described below. For Wi-Fi, the desktop's current LAN URL is
`http://192.168.50.180:8765`; configure it under **Notes → Open or create document**
after allowing the port through the desktop firewall. Addresses remain
configurable because DHCP can change them; `10.0.2.2` is for the emulator only.

The Android app starts its local command proxy at `127.0.0.1:8766`. Its spawned
CLI uses that proxy automatically. Desktop agents can run their own cached
replica synchronized with the primary host:

```sh
nix-shell
apps/cli/build.sh --android
apps/cli/notemaker --state ~/.local/state/notemaker-desktop --document notes serve 8766 http://127.0.0.1:8765
# In another nix-shell:
apps/cli/notemaker --proxy-file ~/.local/state/notemaker-desktop/proxy.json --document notes --actor agent-1 inspect
```

To access the tablet's replica over USB, forward its local port and copy its
private connection descriptor using the authorized debugging connection:

```sh
adb -s <tablet-serial> forward tcp:18767 tcp:8766
mkdir -p .test-artifacts/tablet-proxy
adb -s <tablet-serial> exec-out run-as com.notemaker cat files/proxy.json > .test-artifacts/tablet-proxy/proxy.json
chmod 600 .test-artifacts/tablet-proxy/proxy.json
python3 - <<'PY'
import json
from pathlib import Path
p = Path('.test-artifacts/tablet-proxy/proxy.json')
d = json.loads(p.read_text())
d['url'] = 'http://127.0.0.1:18767'
p.write_text(json.dumps(d))
PY
apps/cli/notemaker --proxy-file .test-artifacts/tablet-proxy/proxy.json --document notes --actor agent-1 inspect
```

The token changes when the service restarts; recopy the descriptor then and keep
it private. Android can stop background execution; force-stop ends the proxy.
Reopen the app to resume synchronization. See [proxy protocol](apps/cli/README.md#local-proxy).

For synchronization over USB with the desktop's existing server, run
`adb -s <tablet-serial> reverse tcp:8765 tcp:8765` and set the tablet's server URL
to `http://127.0.0.1:8765`. This is the current physical-tablet setup and works
with the desktop's firewall. Recreate the tunnel after reconnecting USB. Unplugged
editing remains local until the server is reachable again. For Wi-Fi sync, use
the desktop's LAN URL and allow TCP 8765 on its LAN firewall interface.

Local state consists of a baseline `document-*.json` plus its `.json.journal`
file. Preserve both when backing up, with writers stopped or the document locked.
Use CLI `log` to inspect live state. Do not use an older app/CLI to write a migrated
replica. Existing notes are migrated without clearing them.

### Performance regression check

```sh
nix-shell --run 'apps/cli/build.sh --android && apps/android/build.sh'
nix-shell --run 'python3 tests/proxy_checks.py'
nix-shell --run 'python3 tests/performance.py'
```

The performance check boots an isolated API 36 emulator on port 5586 and draws
1,000 strokes with 50 pressure-bearing points each. It measures event handler,
UI dispatch, input-to-draw and actual window-frame durations at strokes 1–100,
401–500 and 901–1000. It enforces absolute p95 limits (16/50/100/100 ms), a
500 ms maximum, and growth limits relative to the first window (2× plus
8/20/25/25 ms). It also bounds the save queue, compares incremental state with
full replay, and verifies all strokes survive a process restart. Reports are in
`.test-artifacts/performance/`. The emulator is stopped on completion. These
budgets are regression gates, not physical E Ink pen-latency measurements.
The benchmark uses the host GPU with Vulkan disabled and requires a working
graphical session and host graphics driver. `shell.nix` supplies the GL loader;
`NOTEMAKER_TEST_GPU` can override the renderer. The usual emulator launchers
retain their software-rendering defaults for functional tests.

### Agent chats on BOOX

Tap **Agent** in the top bar to open a popup with **New chat**, the five most
recent chats across all documents, and **More chats**. Choose the harness, model,
and effort when starting a chat. Defaults are **Pi / openai / GPT-6-luna / medium**;
**mock** streams a deterministic reply and sample tool activity without reading
or editing notes. Tool rows show filenames or commands; tap one for readable
arguments and results. The popup shrinks above the keyboard while composing,
keeping the message field and Send button visible, and expands when it closes.

Replies stream while the agent works. Closing the popup or disconnecting the
tablet does not stop the agent; reopen the chat to catch up. **Stop** cancels the
run but retains completed edits. Chats persist on the server; unsent drafts stay
on the device. When a send fails, **Retry send** reuses its request ID so an
accepted request is not executed twice. A server restart marks running chats
interrupted; review their activity before requesting further edits. New chats
can use different harness/model settings; existing chats retain their selection.

Run from this checkout on the development computer:

```sh
nix-shell
apps/cli/build.sh
apps/android/build.sh
python3 apps/server/server.py --port 8765 --database notemaker.sqlite3 \
  --pi-command /home/tyler/.npm-global/bin/pi
```

Use the existing primary database when restarting the primary server. The CLI
JAR must be built before Pi can edit notes. The Nix shell supplies Node.js and
Bubblewrap; Pi is the already installed `@earendil-works/pi-coding-agent` CLI
(tested with its RPC `agent_settled` and extension hook APIs). `--pi-command`
can select another installation. The mock needs no provider credentials.

Pi runs inside Bubblewrap in this repository, with read/write access to the
checkout, read-only runtime dependencies, a private temporary home, and network
access to the server and provider. No fallback runs Pi outside the sandbox.
The default upstream is `http://127.0.0.1:8765` (or the chosen server port).
`--agent-root`, `--agent-data`, and `--agent-upstream` configure these locations.
Agent data defaults to `.agent-data` beside the database and is ignored by Git.
It contains a shared CLI replica, per-chat Pi sessions, and private credential
copies. Pi uses the existing CLI skill to discover note content and perform
permission-free edits under a stable chat-specific actor, then synchronizes them.

At startup the server copies `auth.json` and `models.json`, if present, from
`$PI_CODING_AGENT_DIR` or `~/.pi/agent` into private agent data. OAuth refreshes
update the dedicated copy. A newer host file is recopied on server startup;
reauthenticate in Pi and restart the server if those credentials expire.
Credentials, transcript files, and replica data must stay outside committed files.
Back up the server database and agent data together. This feature runs on the
existing trusted-LAN server; it does not add login/authentication.

The model selector initially includes the two Luna models exposed by the
installed Pi catalog. Override its options with `--chat-models FILE.json`:

```json
[
  {"provider":"openai","model":"gpt-6-luna","efforts":["low","medium","high"]},
  {"provider":"openai","model":"gpt-5.6-luna","efforts":["low","medium","high"]}
]
```

The first model is the default, with medium effort when supported. Configure
only provider/model combinations available and authenticated in the selected Pi
installation. Runs are limited to one per chat, four globally, and 30 minutes.
See [chat protocol](apps/server/agent/PROTOCOL.md) and
[ADR 0006](docs/adr/0006-server-agent-chats.md).

```sh
nix-shell --run 'python3 tests/chat_checks.py'
nix-shell --run 'apps/android/build.sh && python3 tests/chat_ui.py'
# Opt-in: uses the real authenticated provider and creates an isolated test note.
nix-shell --run 'python3 tests/pi_chat.py'
```

The HTTP checks cover streaming, reconnect cursors, idempotent sends, cancellation,
history pagination, restart recovery, and a real sandboxed CLI edit/sync. The UI
check uses an isolated emulator on port 5586 and a temporary mock server; it
saves screenshots under `.test-artifacts/chat-ui/`. The live Pi check uses a
temporary server/database and verifies streamed hooks and CLI-generated edits.
