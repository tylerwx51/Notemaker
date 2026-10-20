# Shared JSON parser fast path — NoteAir6C, 2026-10-20

The checkpointed cold-start trace attributed 724 ms to reading, decoding,
and parsing a 2.39 MiB base; applying its 389 events took only 10 ms. The
shared strict JSON parser now scans numeric grammar directly, avoids a fresh
literal array for each scalar, and creates a string builder only for escaped
strings. It still returns `Long` for integers and `Double` for decimals or
exponents. The replica and wire formats did not change.

The same copied note was opened 12 times per isolated-package run on the
physical NoteAir6C, with two process-cold warmups omitted. A disposable
offline proxy config identified the note so discovery did not parse the base
twice. The installed personal app and note were untouched.

| Per-run median | Before parser change | Fast parser 1 | Fast parser 2 |
| --- | ---: | ---: | ---: |
| Android launch, checkpointed base | 1,388 ms | 1,171 ms | 1,165 ms |
| Base read/decode/parse | 724 ms | 486 ms | 486 ms |
| Process CPU | 2.40 s | 2.15 s | 2.16 s |
| Post-launch PSS | 171 MiB | 166 MiB | 163 MiB |

The 216–223 ms launch improvement is close to the 238 ms reduction in the
measured base segment. A separate original-WAL run on the same candidate
launched in 1,360 ms median versus 1,660 ms in the prior baseline; it still
reads 5.22 MiB of logical data rather than the checkpointed 2.92 MiB. This
suggests the parser also reduces WAL replay cost, though the WAL comparison
has one run per build.

The CLI and proxy suites passed, including invalid-number cases and a new
escaped-string round trip through actual CLI calls. A physical 1,000-stroke
run also passed its restart and full-replay parity checks. Input-to-draw p95
was 69.7 ms, near the earlier 68.6–69.0 ms range, and final-window frame p95
was 91.7 ms. It wrote 37.9 MiB charged bytes and ended at 267 MiB PSS;
different run conditions and one sample do not establish a resource saving.

The first 1,000-stroke attempt reached every stroke but failed the harness's
fixed 500 ms final-draw assertion. The log also contains an earlier run with
the same late-draw failure. The harness now waits up to three seconds for a
natural final draw and reports the pending stroke if it still fails; it does
not force an extra draw. The second run passed. These measurements are
storage-warm and do not include E Ink scanout. Note-free aggregate values and
APK hashes are in [`json-parser-fast-path.json`](json-parser-fast-path.json);
raw copied-note artifacts remain private under `.test-artifacts/hillclimb/`.
