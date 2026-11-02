#!/usr/bin/env bash
# Build a realistic replica directory with the Java (JVM) CLI.
# Usage: make_state.sh STATE_DIR   (run inside nix-shell from any directory)
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
STATE=$1
CLI="${NOTEMAKER_JAVA_CLI:?parity.sh sets NOTEMAKER_JAVA_CLI to the Java CLI}"
FIX="$ROOT/packages/core/tests/parity/fixtures"
mkdir -p "$STATE"
"$CLI" --state "$STATE" --document demo serve 0 >"$STATE.serve.log" 2>&1 &
SERVE=$!
trap 'kill $SERVE 2>/dev/null || true; wait $SERVE 2>/dev/null || true' EXIT
for _ in $(seq 1 100); do [ -s "$STATE/proxy.json" ] && break; sleep 0.1; done
export NOTEMAKER_PROXY_FILE="$STATE/proxy.json"
n() { local doc=$1 actor=$2; shift 2; "$CLI" --document "$doc" --actor "$actor" "$@" >/dev/null; }
stroke() { # id brush width color material points-json
  printf '{"stroke":"%s","layer":"base","page":0,"brush":"%s","width":%s,"color":%s,"material":%s,"points":%s}' "$@"
}
n demo agent-1 document create pages
n demo agent-1 diagram add "$ROOT/apps/cli/examples/diagram.json"
i=0
for brush in pen pencil marker brush oily fountain charcoal neoBrush latinCalligraphy asiaCalligraphy square; do
  pts="[[$((40 + i * 5)),40,0.2],[$((60 + i * 5)),55.5,0.6],[$((90 + i * 5)),48,0.9],[$((120 + i * 5)),80.25,1],[$((150 + i * 5)),70,0.4]]"
  stroke "s-$brush" "$brush" $((2 + i % 7)) -16777216 $((i * 9)) "$pts" | n demo human stroke put -
  i=$((i + 1))
done
n demo agent-2 svg add "$FIX/svg/good-matplotlib-bar.svg" - <<<'{"object":"chart","x":100,"y":900,"w":400,"h":300}'
n demo agent-2 apply - <<'JSON'
[{"kind":"object","object":"note-1","layer":"base","page":1,"shape":"text","x":10,"y":20,"w":300,"h":60,"color":-16777216,"width":1,"fontSize":22,"text":"Page two\nsecond line","alignment":"center"},
 {"kind":"object","object":"star-1","layer":"base","page":0,"shape":"star","x":700,"y":700,"w":120,"h":120,"color":-65536,"width":3,"fontSize":24,"text":"","fill":1090519295,"rotation":12.5}]
JSON
n demo agent-1 object update app - <<<'{"scope":{"layer":"architecture","page":0},"x":150,"text":"Renamed app"}'
n demo agent-1 object hide server architecture 0
n demo agent-1 undo
n demo agent-1 undo
n demo agent-1 redo
n demo human layer put - <<<'{"layer":"notes","name":"Notes","force":false}'
n demo human layer hide notes
n demo human settings - <<<'{"hidden":["notes"],"selected":"base","page":1,"undo":["x"],"redo":[]}'
n demo human append - <<<'{"kind":"stroke","stroke":"raw-1","layer":"base","page":0,"width":4,"color":-16776961,"points":[[300,300,0.5],[340,320,0.5],[380,300,0.5]]}'
n demo human erase - <<<'{"mode":"partial","points":[[330,290],[330,330]],"radius":8}'
n demo human fill - <<<'{"object":"star-1","color":1073807104}'
n demo human template add - <<<'{"template":"ruled","spacing":100}'
n demo human checkpoint
stroke s-after pen 3 -16777216 50 '[[10,10,0.5],[20,30,0.5]]' | n demo human stroke put -
n demo agent-2 undo
n notes human document create fixed
pts=""
for j in $(seq 1 14); do
  pts="${pts:+$pts,}[$((j * 7)),$((j * j % 50)).5,0.$((j % 9 + 1))]"
  printf '{"kind":"stroke","stroke":"grow","layer":"base","page":0,"width":5,"color":-16777216,"points":[%s]}' "$pts" | n notes human append -
done
n notes human checkpoint
printf '{"kind":"stroke","stroke":"grow","layer":"base","page":0,"width":5,"color":-16777216,"points":[%s,[1,1,1]]}' "$pts" | n notes human append -
n gone human document create
stroke g1 pen 3 -16777216 50 '[[1,1,0.5]]' | n gone human stroke put -
n gone human document delete
printf 'demo\nnotes\ngone\n' >"$STATE/DOCUMENTS"
