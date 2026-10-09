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
