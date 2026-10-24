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
    abiVersions = [ "x86_64" ];
    includeSources = false;
    includeNDK = false;
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
    pkgs.docker pkgs.rootlesskit pkgs.slirp4netns pkgs.fuse-overlayfs rootlessDocker ];
  ANDROID_HOME = sdk;
  ANDROID_SDK_ROOT = sdk;
  JAVA_HOME = pkgs.jdk17.home;
  NOTEMAKER_JSVG = (import ./apps/cli/dependencies.nix { inherit (pkgs) fetchurl; }).jsvg;
  NOTEMAKER_ANDROIDSVG = (import ./apps/android/dependencies.nix { inherit (pkgs) fetchurl; }).androidsvg;
  NOTEMAKER_GL_LIBRARY_PATH = "${pkgs.libglvnd}/lib";
  FONTCONFIG_FILE = pkgs.makeFontsConf { fontDirectories = [ pkgs.dejavu_fonts ]; };
  shellHook = ''
    export XDG_RUNTIME_DIR="''${XDG_RUNTIME_DIR:-$HOME/.cache/notemaker/docker-runtime}"
    export DOCKER_HOST="unix://$XDG_RUNTIME_DIR/docker.sock"
    echo "Android 16 ready: boox-emulator (GUI), boox-emulator-headless (automated testing), adb"
    echo "Rootless Docker: start notemaker-dockerd in another shell; socket is $DOCKER_HOST"
  '';
}
