#!/usr/bin/env bash
# Java/Rust parity check for packages/core.
#
#   packages/core/tests/parity.sh                    build both, run every comparison
#   packages/core/tests/parity.sh --update-snapshot  also refresh the committed Java
#                                                    snapshot used by `cargo test`
#
# Runs inside the root nix-shell (re-executes itself there when needed).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../../.." && pwd)
if [ -z "${IN_NIX_SHELL:-}" ]; then exec nix-shell "$ROOT/shell.nix" --run "bash '$0' $*"; fi
cd "$ROOT"
HERE=packages/core/tests/parity
F=$ROOT/$HERE/fixtures
UPDATE=${1:-}
WORK=$(mktemp -d /tmp/notemaker-parity.XXXXXX)
echo "work directory: $WORK"
status=0

echo "--- build"
# The Java reference implementation was removed from the tree after the Rust
# port; extract it from the last Java commit (override with NOTEMAKER_JAVA_REF).
JAVA_REF=${NOTEMAKER_JAVA_REF:-0534499}
mkdir -p "$WORK/java"
git -C "$ROOT" archive "$JAVA_REF" packages/core/src/com apps/cli/src/com apps/cli/jvm | tar -x -C "$WORK/java"
J=$WORK/java
javac -nowarn --release 8 -cp "$NOTEMAKER_JSVG" -d "$WORK/cli-classes" $J/packages/core/src/com/notemaker/core/*.java $J/apps/cli/src/com/notemaker/cli/*.java $J/apps/cli/jvm/com/notemaker/cli/*.java 2>&1 | grep -v '^Note:' || true
printf '#!/usr/bin/env bash\nexec java -Djava.awt.headless=true -cp "%s:%s" com.notemaker.cli.Main "$@"\n' "$WORK/cli-classes" "$NOTEMAKER_JSVG" >"$WORK/java-cli"
chmod +x "$WORK/java-cli"
export NOTEMAKER_JAVA_CLI="$WORK/java-cli"
javac -nowarn -d "$WORK/classes" $J/packages/core/src/com/notemaker/core/*.java "$HERE/Parity.java" 2>&1 | grep -v '^Note:' || true
cargo build -q --release -p notemaker_core --example parity
RUST=${CARGO_TARGET_DIR:-$ROOT/target}/release/examples/parity
java_driver() { java -cp "$WORK/classes" Parity "$F" "$@"; }
rust_driver() { "$RUST" "$F" "$@"; }
compare() { python3 "$HERE/compare.py" "$1" "$2" "$HERE/expected/known-differences.txt" || status=1; }

echo "--- fixture sections (json, validation, assets, svg, geometry, scenes/render, actions)"
java_driver all >"$WORK/java-all.txt"
rust_driver all >"$WORK/rust-all.txt"
compare "$WORK/java-all.txt" "$WORK/rust-all.txt"

echo "--- deterministic writes (journal + checkpoint bytes, UUIDs canonicalized)"
java_driver write "$WORK/write-java" >"$WORK/java-write.txt"
rust_driver write "$WORK/write-rust" >"$WORK/rust-write.txt"
compare "$WORK/java-write.txt" "$WORK/rust-write.txt"

echo "--- each implementation reads the other's checkpointed write directory"
java_driver read "$WORK/write-rust" parity-doc >"$WORK/java-read-rustwrite.txt"
rust_driver read "$WORK/write-java" parity-doc >"$WORK/rust-read-javawrite.txt"
compare "$WORK/java-read-rustwrite.txt" "$WORK/rust-read-javawrite.txt"

echo "--- Java CLI writes a replica; Java and Rust read it"
bash "$HERE/make_state.sh" "$WORK/state-java"
mapfile -t DOCS <"$WORK/state-java/DOCUMENTS"
java_driver read "$WORK/state-java" "${DOCS[@]}" >"$WORK/java-read.txt"
rust_driver read "$WORK/state-java" "${DOCS[@]}" >"$WORK/rust-read.txt"
compare "$WORK/java-read.txt" "$WORK/rust-read.txt"

echo "--- Rust appends to the Java replica; Java and Rust read it back"
cp -r "$WORK/state-java" "$WORK/state-mixed"
rust_driver mutate "$WORK/state-mixed" demo >"$WORK/rust-mutate.txt"
rust_driver mutate "$WORK/state-mixed" notes >>"$WORK/rust-mutate.txt"
java_driver read "$WORK/state-mixed" "${DOCS[@]}" >"$WORK/java-read-mixed.txt"
rust_driver read "$WORK/state-mixed" "${DOCS[@]}" >"$WORK/rust-read-mixed.txt"
compare "$WORK/java-read-mixed.txt" "$WORK/rust-read-mixed.txt"
echo "--- Java appends to the Rust-modified replica; both read it back"
java_driver mutate "$WORK/state-mixed" demo >"$WORK/java-mutate.txt"
java_driver read "$WORK/state-mixed" "${DOCS[@]}" >"$WORK/java-read-mixed2.txt"
rust_driver read "$WORK/state-mixed" "${DOCS[@]}" >"$WORK/rust-read-mixed2.txt"
compare "$WORK/java-read-mixed2.txt" "$WORK/rust-read-mixed2.txt"

if [ "$UPDATE" = --update-snapshot ]; then
  echo "--- updating committed Java snapshot"
  python3 "$HERE/snapshot.py" "$WORK/java-all.txt" "$HERE/expected"
  rm -rf "$F/state-java"
  mkdir -p "$F/state-java"
  cp "$WORK"/state-java/document-* "$WORK/state-java/DOCUMENTS" "$F/state-java/"
  java_driver read "$F/state-java" "${DOCS[@]}" >"$HERE/expected/read.txt"
fi
if [ $status = 0 ]; then echo "PARITY OK"; [ -n "${KEEP:-}" ] || rm -rf "$WORK"; else echo "PARITY DIFFERENCES (outputs kept in $WORK)"; fi
exit $status
