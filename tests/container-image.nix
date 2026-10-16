{ pkgs ? import <nixpkgs> {} }:

pkgs.dockerTools.buildImage {
  name = "notemaker-tests";
  tag = "dev";
  copyToRoot = pkgs.buildEnv {
    name = "notemaker-test-container-root";
    paths = [ pkgs.bash pkgs.coreutils ];
    pathsToLink = [ "/bin" ];
  };
  extraCommands = ''
    mkdir -p usr/bin
    ln -s /bin/env usr/bin/env
  '';
  config = {
    WorkingDir = "/workspace";
    Labels = {
      "org.opencontainers.image.title" = "NoteMaker test environment";
      "org.opencontainers.image.description" = "Uses the pinned project Nix store as its runtime.";
    };
  };
}
