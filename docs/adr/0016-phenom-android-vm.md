# ADR 0016: Do not target Phenom II for the Android emulator

Status: accepted (2026-10-29)

## Context

The Phenom II X4 955 server has AMD-V/KVM, 8 GiB RAM, and a proprietary-driver
GTX 1060. It has no graphical desktop. NoteMaker requires Android API 26 or
newer, and the repository's regular emulator uses Android 16 / API 36.

## Decision

Keep Android emulator development and verification on a newer host or the
physical tablet. Do not add a Phenom-specific VM launcher or deploy a
persistent display service for this CPU.

The lack of a window manager is manageable: a minimal Xorg server on `:99`
provided NVIDIA OpenGL ES rendering to the headless emulator. The CPU is the
blocking constraint. The API 36 x86_64 guest requires SSSE3 and SSE4.x, which
the Phenom II lacks; its `init` traps on an invalid opcode and the kernel
panics. An API 26 x86 guest also requires SSSE3 and repeatedly loses `adbd`,
`logd`, and keymaster to signal 4. CPU translation with QEMU TCG got API 36
through early service startup but did not complete boot after more than an
hour while consuming substantial host resources.

## Consequences

The server can host repository code and non-emulator services, but its GTX
1060 does not make these Android images usable with KVM. Revisit this only
with a compatible Android image that supports the app's minimum API and the
Phenom CPU, or after replacing the CPU/host. A future headless GPU setup needs
an X server or another proven graphics display backend; it does not need
Plasma or GNOME.
