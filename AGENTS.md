# NoteMaker agent instructions

## Project purpose

This repository is a monorepo for applications related to an AI-integrated E Ink
display tablet. Tablet applications may communicate with this development
computer or the Phenom II home server. Keep application boundaries clear and
share protocols and reusable code when there is an actual common need.

Applications live in `apps/android`, `apps/server`, and `apps/cli`. The shared
Rust replica, action model, and rendering traversal live in `packages/core`
(crate `notemaker_core`). Keep the CLI available on both NixOS and Android (one
Rust binary; the APK packages it). The Android app is Rust over JNI; its Java
shim in `apps/android/shim` only forwards framework callbacks and holds no logic.

Every new document/drawing action must also be exposed through the CLI, with
structured input/output and appropriate verification using actual CLI calls.
Every new app navigation or editor control must also have a semantic live-session
CLI action in `UiActions`, with structured input/output and an actual CLI check.
Update the discoverable action registry and CLI documentation with that control.
App and CLI edits go through shared local state; neither may bypass it with
direct server mutations. Local editing must work with the server unavailable.
Preserve actor-specific histories and atomic actions, including entire diagrams.
Do not add AI agent orchestration unless requested.

## Development environment

- Manage all required development tools through the root `shell.nix`. Add tools
  there when needed instead of relying on globally installed tools or ad hoc
  package installation.
- Run builds, checks, and development commands inside `nix-shell`, or through
  `nix-shell --run '<command>'`.
- Keep application dependency manifests and lockfiles alongside their owning
  applications; `shell.nix` supplies their development toolchains.
- Update `README.md` when setup, build, run, or test commands change.
- Preserve the existing Android SDK and emulator launchers unless the task
  requires changing them. See `README.md` for their usage and limitations.

## Tablet considerations

- Design interfaces with E Ink refresh behavior, readability, and limited
  animation in mind. Avoid unnecessary redraws.
- Account for intermittent connectivity when designing tablet/host workflows.
- Keep host addresses and AI service configuration configurable. Keep credentials
  outside committed source files.
- Android emulator checks can validate ordinary application behavior and layout;
  E Ink refresh, BOOX-specific behavior, and stylus characteristics require
  validation on actual hardware.

## Home server and network

Read `../computer-it/NETWORK_OPERATING_GUIDE.md` for the network inventory,
connection details, and server operating context before server-related work.
That guide is the source of truth; avoid duplicating its full inventory here.

The guide records SSH access as `ssh tyler@192.168.50.129`. Addresses are assigned
automatically and may change. SSH inspection is available when needed; inspect
existing configuration before making server changes.

The server is an x86_64 AMD Phenom II X4 955 with approximately 8 GiB RAM running
NixOS. Choose server workloads with these limits in mind, verify CPU instruction
requirements for dependencies, and do not assume GPU acceleration or sufficient
capacity for large local AI models. Preserve existing services during project
work. A request to develop locally does not by itself require remote deployment.

## Verification

Run checks appropriate to the affected application through the Nix development
environment. Record what was verified and any hardware or remote checks that
remain untested. Keep documentation aligned with implemented behavior.

## Misc
Keep your ADRs up to date.
