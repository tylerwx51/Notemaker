#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
rm -rf build/classes build/dex
mkdir -p build/classes build/dex
sdk_tools="$ANDROID_HOME/build-tools/36.0.0"
"$sdk_tools/aapt2" link -o build/base.apk --manifest AndroidManifest.xml -I "$ANDROID_HOME/platforms/android-36/android.jar"
javac -source 8 -target 8 -cp "$ANDROID_HOME/platforms/android-36/android.jar" -d build/classes src/com/notemaker/*.java
find build/classes -name '*.class' > build/classes.list
"$sdk_tools/d8" --lib "$ANDROID_HOME/platforms/android-36/android.jar" --output build/dex @build/classes.list
cp build/base.apk build/notemaker-unsigned.apk
(cd build/dex && zip -q ../notemaker-unsigned.apk classes.dex)
"$sdk_tools/zipalign" -f 4 build/notemaker-unsigned.apk build/notemaker.apk
if [ ! -f build/debug.keystore ]; then
  keytool -genkeypair -keystore build/debug.keystore -storepass android -keypass android -alias debug -dname 'CN=NoteMaker Development' -keyalg RSA -validity 10000 >/dev/null 2>&1
fi
"$sdk_tools/apksigner" sign --ks build/debug.keystore --ks-pass pass:android build/notemaker.apk
