#!/usr/bin/env bash
# Builds build/notemaker.apk from the Rust port: libnotemaker.so (app),
# libnotemaker_cli.so (the `notemaker` CLI executable) and the Java shim.
set -euo pipefail
cd "$(dirname "$0")"
root="$(cd ../.. && pwd)"
sdk_tools="$ANDROID_HOME/build-tools/36.0.0"
android_jar="$ANDROID_HOME/platforms/android-36/android.jar"
abis=(${NOTEMAKER_ABIS:-arm64-v8a x86_64})
profile="${NOTEMAKER_PROFILE:-release}"
rm -rf build/classes build/dex build/lib
mkdir -p build/classes build/dex build/lib
targets=()
for abi in "${abis[@]}"; do targets+=(-t "$abi"); done
(cd "$root" && cargo ndk "${targets[@]}" -P 26 -o apps/android/build/lib build --profile "$profile" -p notemaker_android -p notemaker_cli)
for abi in "${abis[@]}"; do
  case "$abi" in arm64-v8a) triple=aarch64-linux-android ;; x86_64) triple=x86_64-linux-android ;; *) echo "unsupported ABI $abi" >&2; exit 2 ;; esac
  # Android only extracts lib*.so entries; the CLI executable rides along under that name.
  cp "${CARGO_TARGET_DIR:-$root/target}/$triple/$profile/notemaker" "build/lib/$abi/libnotemaker_cli.so"
  rm -f "build/lib/$abi/libnotemaker_cli.so.d"
  # Keep symbols in the target directory for crash symbolication; ship stripped copies.
  "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-strip" --strip-debug build/lib/$abi/*.so
done
"$sdk_tools/aapt2" link -o build/base.apk --manifest AndroidManifest.xml -I "$android_jar"
javac -nowarn --release 8 -cp "$android_jar" -d build/classes shim/com/notemaker/*.java
find build/classes -name '*.class' > build/classes.list
"$sdk_tools/d8" --min-api 26 --lib "$android_jar" --output build/dex @build/classes.list
cp build/base.apk build/notemaker-unsigned.apk
mkdir -p build/dex/assets/licenses
python3 tools/licenses.py build/dex/assets/licenses/third-party.txt
(cd build/dex && zip -q ../notemaker-unsigned.apk classes.dex assets/licenses/third-party.txt)
(cd build && zip -q -0 -r notemaker-unsigned.apk lib)
"$sdk_tools/zipalign" -f -P 16 4 build/notemaker-unsigned.apk build/notemaker.apk
if [ ! -f build/debug.keystore ]; then
  keytool -genkeypair -keystore build/debug.keystore -storepass android -keypass android -alias debug -dname 'CN=NoteMaker Development' -keyalg RSA -validity 10000 >/dev/null 2>&1
fi
"$sdk_tools/apksigner" sign --ks build/debug.keystore --ks-pass pass:android build/notemaker.apk
