# Invoked by the Nix-provided boox-emulator-arm64 launcher.
for arg in "$@"; do
  case "$arg" in
    -help|-help-*|-version|-list-avds|-accel-check|-webcam-list)
      exec "$ANDROID_HOME/emulator/emulator" "$@"
      ;;
  esac
done
export ANDROID_AVD_HOME="${ANDROID_AVD_HOME:-$HOME/.android/avd}"
mkdir -p "$ANDROID_AVD_HOME"
avd_name=notemaker-boox-air6c-arm64-api36
if [[ ! -f "$ANDROID_AVD_HOME/$avd_name.ini" ]]; then
  printf 'no\n' | avdmanager create avd \
    --name "$avd_name" \
    --package 'system-images;android-36;google_apis;arm64-v8a' \
    --path "$ANDROID_AVD_HOME/$avd_name.avd"
  # Replace generated values rather than leaving duplicate INI keys.
  sed -i -E '/^(hw\.(lcd\.(width|height|density)|ramSize|keyboard|mainKeys|initialOrientation|sdCard)|disk.dataPartition.size|showDeviceFrame)=/d' \
    "$ANDROID_AVD_HOME/$avd_name.avd/config.ini"
  cat >> "$ANDROID_AVD_HOME/$avd_name.avd/config.ini" <<'CONFIG'
hw.lcd.width=1860
hw.lcd.height=2480
hw.lcd.density=300
hw.ramSize=4096
hw.keyboard=yes
hw.mainKeys=no
hw.initialOrientation=portrait
hw.sdCard=no
disk.dataPartition.size=8G
showDeviceFrame=no
CONFIG
fi

# The frontend rejects API >=28 ARM64 images on x86_64. Its bundled engine
# works with virt instead of its legacy ranchu default.
engine=qemu-system-aarch64
port=5574
previous=
for arg in "$@"; do
  if [[ "$previous" == -port ]]; then port="$arg"; fi
  if [[ "$arg" == -no-window ]]; then engine=qemu-system-aarch64-headless; fi
  if [[ "$arg" == -qemu || "$arg" == -ports || "$arg" == -avd || "$arg" == @* ]]; then
    echo 'The launcher owns the AVD and QEMU configuration; use -port to change the console port.' >&2
    exit 2
  fi
  previous="$arg"
done
if [[ ! "$port" =~ ^[0-9]+$ ]] || (( port < 5554 || port > 5682 || port % 2 != 0 )); then
  echo 'Use an even emulator console port between 5554 and 5682.' >&2
  exit 2
fi
exec {lock_fd}>"$ANDROID_AVD_HOME/$avd_name.launcher.lock"
if ! flock -n "$lock_fd"; then
  echo 'This ARM64 AVD is already running through the launcher.' >&2
  exit 1
fi
if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
  echo "Emulator console port $port is already in use; choose another -port." >&2
  exit 1
fi
echo 'ARM64 Android 16 uses software CPU emulation; boot and apps are slower than the x86_64 emulator.' >&2
# Even with -accel off, the ARM64 engine on x86_64 can select KVM's guest-memory
# translator merely because /dev/kvm is usable. Its uninitialized mappings
# then crash goldfish address-space pings. Mask KVM only in this process's mount
# namespace so the engine consistently selects TCG's memory translator.
command=("$ANDROID_HOME/emulator/qemu/linux-x86_64/$engine")
if [[ -e /dev/kvm ]]; then
  command=(bwrap --die-with-parent --bind / / --dev-bind /dev /dev --ro-bind /dev/null /dev/kvm "${command[@]}")
fi
"${command[@]}" \
  -avd "$avd_name" -port "$port" -accel off -cores 4 -memory 4096 \
  -no-boot-anim -no-audio -no-snapshot -gpu swiftshader_indirect \
  -feature -Vulkan "$@" -qemu -machine virt -cpu cortex-a57 &
engine_pid=$!
setup_pid=
cleanup() {
  if [[ -n "$setup_pid" ]]; then kill "$setup_pid" 2>/dev/null || true; fi
  kill "$engine_pid" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# API 36's ART image relocation fails under this TCG engine. Use ART's
# class-loading fallback before restarting the framework.
# qemu-props cannot set dalvik properties under SELinux, so configure only our
# explicitly selected userdebug emulator through ADB; never select a USB device.
configure_guest() {
  local serial="emulator-$port" actual
  for ((attempt=0; attempt<300; attempt++)); do
    kill -0 "$engine_pid" 2>/dev/null || return 1
    if [[ $(timeout 5 adb -s "$serial" get-state 2>/dev/null) == device ]]; then
      actual=$(timeout 10 adb -s "$serial" emu avd name | tr -d '\r')
      if [[ "${actual%%$'\n'*}" != "$avd_name" ]]; then
        echo "Refusing to configure unexpected AVD on $serial: $actual" >&2
        return 1
      fi
      timeout 30 adb -s "$serial" root
      timeout 30 adb -s "$serial" wait-for-device
      timeout 30 adb -s "$serial" shell 'stop; setprop dalvik.vm.boot-image /system/nonexistent; setprop ro.hw_timeout_multiplier 10; start'
      actual=$(timeout 10 adb -s "$serial" shell getprop dalvik.vm.boot-image | tr -d '\r')
      [[ "$actual" == /system/nonexistent ]] || return 1
      actual=$(timeout 10 adb -s "$serial" shell getprop ro.hw_timeout_multiplier | tr -d '\r')
      [[ "$actual" == 10 ]] || return 1
      timeout 30 adb -s "$serial" unroot
      echo "ARM64 runtime configured on $serial; waiting for Android to finish booting." >&2
      return 0
    fi
    sleep 2
  done
  echo 'ARM64 emulator did not become available over ADB within the startup timeout.' >&2
  return 1
}
configure_guest &
setup_pid=$!
# A failed engine or setup must not leave the other process behind.
wait -n "$engine_pid" "$setup_pid"
if kill -0 "$engine_pid" 2>/dev/null; then
  wait "$setup_pid"
  setup_pid=
  wait "$engine_pid"
fi
