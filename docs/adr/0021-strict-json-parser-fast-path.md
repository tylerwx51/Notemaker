# ADR 0021: Strict JSON parser fast paths

Status: accepted on `perf/json-parser-fast-path`, 2026-10-20.

## Context

The shared Java JSON codec runs in the Android app, JVM CLI, and proxy. A
checkpointed physical-device note spends about 724 ms reading, decoding, and
parsing its 2.39 MiB base, while applying its 389 committed events takes only
about 10 ms. The old parser allocated a `StringBuilder` for every string,
iterated a temporary literal array for every scalar, and rescanned numeric
tokens to validate them after finding their end.

## Decision

Keep the existing strict JSON grammar and value types. Scan whitespace and
numbers directly with character comparisons; choose `true`, `false`, and
`null` by their first character; and return an unescaped string directly from
its substring, creating a builder only when an escape is present. Continue to
reject duplicate object keys, nonfinite or out-of-range numbers, invalid
escapes and controls, and excessive nesting. No replica or wire format changes.

## Evidence and consequences

The CLI and proxy suites passed, including strict-number cases, restart, and
an actual CLI escaped-string round trip. Two physical NoteAir6C checkpointed
cold-start runs reached 1,171 and 1,165 ms median versus 1,388 ms before the
parser change. Base read/decode/parse fell from 724 to about 486 ms. The
original WAL layout also launched in 1,360 ms versus 1,660 ms previously.
A physical 1,000-stroke run passed restart and replay parity with input-to-draw
p95 of 69.7 ms, near prior runs. The parser is shared, so its correctness
remains part of all app and CLI checks. See
`docs/design/performance/json-parser-fast-path.md`.
