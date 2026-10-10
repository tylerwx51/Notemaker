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

The repository has two subprojects:

- `apps/android`: native Android 16 note-taking app for BOOX Note Air6 C.
- `apps/server`: local HTTP server, durable SQLite stroke log, and live browser viewer.

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
computer's firewall if needed. The emulator default is `http://10.0.2.2:8765`.
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

Drawing continues offline. The app saves its event history and upload queue in
private storage and retries automatically. The server stores all committed
operations in SQLite, including intermediate stroke snapshots. Back up the
SQLite database with SQLite's backup facilities or while the server is stopped.
A document's saved sync cursor belongs to that server's database; URL changes
should reach the same server, not a different empty database.

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
