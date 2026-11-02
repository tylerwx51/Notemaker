#!/usr/bin/env bash
# Build the Rust CLI (crate notemaker_cli) and its probe binaries.
# --android also cross-builds the CLI for arm64-v8a and x86_64 with cargo ndk.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release -p notemaker_cli
if [ "${1:-}" = --android ]; then
  cargo ndk -t arm64-v8a -t x86_64 -P 26 build --release -p notemaker_cli
fi
