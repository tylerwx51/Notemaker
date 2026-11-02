# ADR 0015: ARM64 Android 16 testing on an x86_64 host

## Status

Accepted.

## Context

The Note Air6 C runs Android 16 / API 36 on ARM64. The existing x86_64
emulator provides fast application checks but cannot execute ARM64-only native
libraries. Emulator 37.1.11's frontend rejects recent ARM64 images on x86_64.
The distributed ARM64 engine still supports software CPU translation, but
defaults to the legacy `ranchu` machine on this host. Current image/device
configuration needs PCI devices that this machine does not provide.

## Decision

Supply Google's API 36 `arm64-v8a` image alongside x86_64 through `shell.nix`.
Add separate GUI and headless ARM64 launchers that invoke the bundled ARM64
engine directly with `-machine virt -cpu cortex-a57`, using software CPU emulation.
Use a separate persistent AVD, port 5574, four cores, 4 GiB RAM, and the tablet's
1860 × 2480 / 300 dpi display profile. Disable snapshots, audio, and Vulkan.
Keep existing accelerated launchers and automated test defaults unchanged.

Mask `/dev/kvm` with `/dev/null` in a Bubblewrap mount namespace for this
process. With host KVM available, the ARM64 engine's memory translation picks
KVM despite `-accel off`, producing a null ping-info pointer and crashing in
the goldfish address-space device. Masking KVM makes it select the software
translator. Host device permissions and other emulator processes are unchanged.
This requires unprivileged user namespaces. Include GDB in the development
shell for diagnosing emulator failures.

ART in this image crashes in `ClassLinker::AddImageSpaces` when relocating the
boot image under TCG. Disabling only boot-image relocation still crashes when
loading an app image. Set `dalvik.vm.boot-image=/system/nonexistent` to select
ART's class-loading fallback. The launcher configures this on every boot through a
strictly selected emulator ADB serial after checking its AVD name, briefly
roots ADB, restarts the framework, and restores non-root ADB. Guest SELinux
remains enforcing. The emulator's `-prop` transport cannot set this property
under the guest's SELinux policy. Supervise engine and setup processes together
so a failed engine or setup does not leave its sibling running.
Set the guest's `ro.hw_timeout_multiplier=10` before restarting the framework:
the default watchdog timeout kills system_server during software-emulated
home-screen startup. These extended timeouts and ART fallback make
this a functional compatibility target, not a security or performance baseline.

## Consequences

ARM64 code can be tested locally without installing a different OS on the
tablet. Translation is slower and unsuitable for tablet performance claims.
This engine entry point is experimental and must be rechecked on SDK upgrades.
App readiness requires user 0 to reach `RUNNING_UNLOCKED`, in addition to
`sys.boot_completed=1`: software emulation leaves a noticeable gap between
these events, during which installed activities may not resolve.
The image does not contain BOOX firmware or reproduce its hardware services,
E Ink display, or stylus. BOOX-dependent native libraries may still fail even
when their CPU architecture matches. Real hardware remains necessary.

The machine selection comes from the upstream
[emulator glue](https://android.googlesource.com/platform/external/qemu/+/refs/heads/emu-main-dev/android-qemu2-glue/main.cpp);
the legacy [ranchu board](https://android.googlesource.com/platform/external/qemu/+/refs/heads/emu-main-dev/hw/arm/ranchu.c)
provides MMIO transports rather than the required PCI bus.

## Update 2026-11-02

The Java implementation described here was ported one-to-one to Rust and
removed (ADR 0026 CLI, ADR 0029 Android app, `docs/design/rust-port.md`).
The decision stands; JVM, `app_process`, jsvg/androidsvg and `.java` details
now refer to their Rust equivalents (native CLI binary, resvg).
