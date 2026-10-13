#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
rm -rf build/classes build/dex
mkdir -p build/classes build/dex
javac --release 8 -cp "$NOTEMAKER_JSVG" -d build/classes ../../packages/core/src/com/notemaker/core/*.java src/com/notemaker/cli/*.java jvm/com/notemaker/cli/*.java
(cd build/classes && jar xf "$NOTEMAKER_JSVG" && rm -f META-INF/MANIFEST.MF module-info.class)
jar cfe build/notemaker.jar com.notemaker.cli.Main -C build/classes .
if [ "${1:-}" = --android ]; then
  rm -rf build/android
  mkdir -p build/android
  unzip -p "$NOTEMAKER_ANDROIDSVG" classes.jar > build/androidsvg.jar
  javac -source 8 -target 8 -cp "$ANDROID_HOME/platforms/android-36/android.jar:build/androidsvg.jar" -d build/android ../../packages/core/src/com/notemaker/core/*.java src/com/notemaker/cli/*.java android/com/notemaker/cli/*.java
  find build/android -name '*.class' > build/android.list
  "$ANDROID_HOME/build-tools/36.0.0/d8" --min-api 26 --lib "$ANDROID_HOME/platforms/android-36/android.jar" --output build/dex @build/android.list build/androidsvg.jar
  (cd build/dex && zip -q ../notemaker-android.jar classes.dex)
fi
