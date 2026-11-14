# Setup, build, and test

Run everything inside `nix-shell` (or `nix-shell --run '…'`). The root
`shell.nix` provides Rust 1.95 via rustup, the Android SDK/NDK, emulator
launchers, JDK 17, Python, Chromium, and the Dylint lints.

## Build

```sh
apps/cli/build.sh              # host CLI  → target/release/notemaker
apps/cli/build.sh --android    # also cross-build for arm64-v8a and x86_64
apps/android/build.sh          # APK       → apps/android/build/notemaker.apk
```

`apps/android/build.sh` accepts `NOTEMAKER_ABIS` (default `arm64-v8a x86_64`)
and `NOTEMAKER_PROFILE` (default `release`). The APK is signed with a debug
keystore that is generated on first build.

## Server

```sh
python3 apps/server/server.py --port 8765 --database notemaker.sqlite3
```

| Flag | Purpose |
| --- | --- |
| `--host` | Bind address (default `0.0.0.0`) |
| `--agent-data` | Chat sessions and the agent's replica (default: database dir `/.agent-data`) |
| `--pi-command` | Pi executable (default `pi`) |
| `--agent-upstream` | CLI sync URL for the agent (default `http://127.0.0.1:PORT`) |
| `--chat-models FILE.json` | Override provider, model, and effort options |

Keep credentials out of the repo. Pi reads its own provider configuration.
The browser viewer is served at `http://<server>:8765/`.

## Emulator

The emulator profile matches the BOOX Note Air6 C (1860×2480 at 300 dpi,
Android 16).

```sh
boox-emulator                 # GUI
boox-emulator-headless        # automated runs
boox-emulator-arm64           # ARM64, software CPU emulation (slow)
boox-emulator-arm64-headless
adb install -r apps/android/build/notemaker.apk
```

## Tablet

```sh
adb install -r apps/android/build/notemaker.apk
adb reverse tcp:8765 tcp:8765   # if the server runs on this PC
```

Then point the app at the server. Host addresses are configurable and not
compiled in.

## Tests

```sh
python3 tests/run.py --list                       # show the plan
python3 tests/run.py                              # local tier (default)
python3 tests/run.py --tier local --tier emulator # what the pre-commit hook runs
python3 tests/run.py --tier hardware --serial SERIAL --install-tablet --reverse-port 8765
python3 tests/run.py --tier network --topology topology.json
```

- **Tiers:** `local` (no devices), `emulator` (starts its own emulators),
  `hardware` (an explicitly chosen tablet), `network` (an external
  server/proxy topology). Devices are never picked automatically.
- **Artifacts:** screenshots and reports go to `.test-artifacts/`.
- **Containers:** `--containers` needs a Docker daemon; start one with
  `notemaker-dockerd` in another shell.
- **Pre-commit hook:** `git config core.hooksPath .githooks`. The shell puts a
  `jj` wrapper on `PATH`, so `jj commit` runs the hook too.

Other checks:

```sh
cargo test --workspace
cargo clippy --workspace
cargo dylint --all              # project lints (tools/lints)
packages/core/tests/parity.sh   # Rust vs. Java reference render parity
```

Mutation testing: [mutation-testing.md](mutation-testing.md). Test tier design:
[ADR 0008](adr/0008-test-tiers.md).

## Hardware caveats

The emulator checks app behavior and layout. E Ink refresh, BOOX-specific
behavior, and stylus feel can only be checked on a real device.
