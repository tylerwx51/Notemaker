{ pkgs ? import <nixpkgs> {
    config = {
      allowUnfree = true;
      android_sdk.accept_license = true;
    };
  }
}:

let
  android = pkgs.androidenv.composeAndroidPackages {
    platformVersions = [ "36" ];
    buildToolsVersions = [ "36.0.0" ];
    includeEmulator = true;
    includeSystemImages = true;
    systemImageTypes = [ "google_apis" ];
    abiVersions = [ "x86_64" "arm64-v8a" ];
    includeSources = false;
    includeNDK = true;
    ndkVersions = [ "29.0.14206865" ];
    includeCmake = false;
  };
  sdk = "${android.androidsdk}/libexec/android-sdk";
  emulator = pkgs.writeShellApplication {
    name = "boox-emulator";
    runtimeInputs = [ android.androidsdk pkgs.coreutils ];
    text = ''
      export ANDROID_HOME="${sdk}"
      export ANDROID_SDK_ROOT="$ANDROID_HOME"
      export JAVA_HOME="${pkgs.jdk17.home}"
      export ANDROID_AVD_HOME="''${ANDROID_AVD_HOME:-$HOME/.android/avd}"
      mkdir -p "$ANDROID_AVD_HOME"
      avd_name=notemaker-boox-air6c-api36
      if [[ ! -f "$ANDROID_AVD_HOME/$avd_name.ini" ]]; then
        printf 'no\n' | avdmanager create avd \
          --name "$avd_name" \
          --package 'system-images;android-36;google_apis;x86_64' \
          --path "$ANDROID_AVD_HOME/$avd_name.avd"
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
      exec emulator -avd "$avd_name" -no-boot-anim -gpu swiftshader_indirect "$@"
    '';
  };
  headless = pkgs.writeShellApplication {
    name = "boox-emulator-headless";
    runtimeInputs = [ emulator ];
    text = ''
      exec boox-emulator -no-window -no-audio -no-snapshot "$@"
    '';
  };
  arm64Emulator = pkgs.writeShellApplication {
    name = "boox-emulator-arm64";
    runtimeInputs = [ android.androidsdk pkgs.coreutils pkgs.gnused pkgs.bubblewrap pkgs.util-linux ];
    text = ''
      export ANDROID_HOME="${sdk}"
      export ANDROID_SDK_ROOT="$ANDROID_HOME"
      export JAVA_HOME="${pkgs.jdk17.home}"
      export LD_LIBRARY_PATH="${pkgs.libglvnd}/lib:${pkgs.lib.getLib pkgs.systemd}/lib:${pkgs.lib.getLib pkgs.dbus}/lib:''${LD_LIBRARY_PATH:-}"
      export QT_XKB_CONFIG_ROOT="${pkgs.xkeyboard_config}/share/X11/xkb"
      export QTCOMPOSE="${pkgs.lib.getLib pkgs.libx11}/share/X11/locale"
    '' + builtins.readFile ./scripts/boox-emulator-arm64.sh;
  };
  arm64Headless = pkgs.writeShellApplication {
    name = "boox-emulator-arm64-headless";
    runtimeInputs = [ arm64Emulator ];
    text = ''
      exec boox-emulator-arm64 -no-window "$@"
    '';
  };
  dylint = pkgs.callPackage ./tools/nix/dylint.nix { };
  rootlessDocker = pkgs.writeShellApplication {
    name = "notemaker-dockerd";
    runtimeInputs = [ pkgs.docker pkgs.rootlesskit pkgs.slirp4netns pkgs.fuse-overlayfs pkgs.coreutils ];
    text = ''
      export XDG_RUNTIME_DIR="''${XDG_RUNTIME_DIR:-$HOME/.cache/notemaker/docker-runtime}"
      export DOCKER_HOST="unix://$XDG_RUNTIME_DIR/docker.sock"
      data_root="''${NOTEMAKER_DOCKER_DATA:-$HOME/.local/share/notemaker/docker}"
      mkdir -p "$XDG_RUNTIME_DIR" "$data_root"
      chmod 700 "$XDG_RUNTIME_DIR"
      exec rootlesskit --net=slirp4netns --disable-host-loopback --copy-up=/etc --copy-up=/run \
        dockerd --host "$DOCKER_HOST" --data-root "$data_root" --storage-driver=vfs --iptables=false
    '';
  };
in
pkgs.mkShell {
  packages = [ android.androidsdk pkgs.jdk17 (pkgs.python3.withPackages (ps: [ ps.pyyaml ps.hypothesis ps.matplotlib ps.pillow ])) pkgs.chromium pkgs.fontconfig pkgs.dejavu_fonts pkgs.zip pkgs.unzip pkgs.jadx pkgs.apktool emulator headless pkgs.git pkgs.jujutsu pkgs.openssh pkgs.tmux pkgs.iproute2 pkgs.bubblewrap pkgs.nodejs
    pkgs.docker pkgs.rootlesskit pkgs.slirp4netns pkgs.fuse-overlayfs rootlessDocker arm64Emulator arm64Headless pkgs.gdb
    pkgs.rustup pkgs.cargo-ndk pkgs.pkg-config dylint.cargo-dylint dylint.dylint-link pkgs.cargo-mutants ];
  ANDROID_HOME = sdk;
  ANDROID_SDK_ROOT = sdk;
  JAVA_HOME = pkgs.jdk17.home;
  ANDROID_NDK_HOME = "${sdk}/ndk/29.0.14206865";
  # Only packages/core/tests/parity.sh uses jsvg, to build the Java reference CLI.
  NOTEMAKER_JSVG = (import ./apps/cli/dependencies.nix { inherit (pkgs) fetchurl; }).jsvg;
  NOTEMAKER_GL_LIBRARY_PATH = "${pkgs.libglvnd}/lib";
  FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
  shellHook = ''
    export XDG_RUNTIME_DIR="''${XDG_RUNTIME_DIR:-$HOME/.cache/notemaker/docker-runtime}"
    export DOCKER_HOST="unix://$XDG_RUNTIME_DIR/docker.sock"
    # `jj commit` runs the Git pre-commit hook first (Jujutsu has no hooks).
    export PATH="${toString ./.}/.githooks/bin:$PATH"
    echo "Android 16 ready: boox-emulator (GUI), boox-emulator-headless (automated testing), adb"
    echo "ARM64 Android 16: boox-emulator-arm64, boox-emulator-arm64-headless (software CPU emulation)"
    echo "Rootless Docker: start notemaker-dockerd in another shell; socket is $DOCKER_HOST"
  '';
}
