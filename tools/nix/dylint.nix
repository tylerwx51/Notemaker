# cargo-dylint and dylint-link are not in nixpkgs. Built from crates.io with
# their published lockfiles. Keep the version in step with `dylint_linting`
# in tools/lints/notemaker_lints/Cargo.toml.
{ lib, rustPlatform, fetchCrate, pkg-config, openssl, libgit2, zlib, curl }:

let
  version = "6.3.2";
  build = { pname, hash, cargoHash }: rustPlatform.buildRustPackage {
    inherit pname version cargoHash;
    src = fetchCrate { inherit pname version hash; };
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [ openssl libgit2 zlib curl ];
    # dylint's build script embeds a path to an in-repo driver unless it is
    # built from under CARGO_HOME; the vendor dir is under NIX_BUILD_TOP, so
    # this selects the crates.io driver, as `cargo install` would.
    preBuild = "export CARGO_HOME=$NIX_BUILD_TOP";
    # Upstream tests need network access and rustup toolchains.
    doCheck = false;
    meta.license = with lib.licenses; [ asl20 mit ];
  };
in
{
  cargo-dylint = build {
    pname = "cargo-dylint";
    hash = "sha256-ylpUmv8vDyXdCaSsTfqfgN0oKEPZQQ/T0Y+CBbF8rcU=";
    cargoHash = "sha256-HWgKf7nlTZTc2HbCLuLFcTHcx/zufkeYB8V53EeQfiU=";
  };
  dylint-link = build {
    pname = "dylint-link";
    hash = "sha256-3lSKsbybj/Szt3EivfwSZxa+321yt0qaZlVzX0jwYxg=";
    cargoHash = "sha256-OOe11kKOal4WGzOgK9lHkrzP+igBGSnhT/SXv61N2qA=";
  };
}
