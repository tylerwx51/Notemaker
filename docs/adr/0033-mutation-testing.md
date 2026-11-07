# ADR 0033: Mutation testing with per-test attribution

Status: accepted, 2026-11-07.

## Context

Line coverage does not show whether tests would notice a wrong result. We want
to know which code is weakly tested and which tests actually detect faults.
Most behaviour is tested end to end by the Python local tier through the real
CLI and server, not by Rust unit tests. The CLI crate has no unit tests at all.
cargo-mutants can only run `cargo test` or nextest, and it reports only
caught or missed per mutant, not which tests failed.

## Decision

- `shell.nix` provides `cargo-mutants`. We use it only to *generate* Rust
  mutants (`cargo mutants --list --json`), so the mutation operators match
  upstream.
- `tools/mutation/python_mutants.py` generates mutants for the Python server
  in the same JSON shape, using a small operator set. There is no packaged
  Python mutation tool in nixpkgs.
- `tools/mutation/run_mutants.py` samples mutants evenly per file and applies
  each one in an isolated worker copy. For core and CLI mutants it builds the
  release CLI and runs `cargo test --workspace` together with every
  local-tier suite. `unittest_json.py` records per-test outcomes. Android
  mutants run only `cargo test -p notemaker_android`, because the emulator
  tier is too slow per mutant.
- **Noise control.** Unmutated baseline runs set the list of stable tests. If
  only a few tests fail, the failing suites rerun with the mutant still
  applied, and a failure counts only if it repeats.
- **Memory.** Each command runs in its own systemd user scope with a memory
  cap, because some mutants allocate until the kernel's global OOM killer
  takes down unrelated processes.
- **Restoring files.** Worker copies are restored by content with fresh mtimes.
  Otherwise cargo can reuse a stale build of a mutated file.
- Mutation testing runs on demand. It is not part of `tests/run.py` or the
  pre-commit hook: a useful sample takes hours.

## Consequences

- The first campaign (1,767 sampled mutants, about 6 hours) is reported in
  `docs/mutation-testing.md`, with generated tables in
  `docs/mutation-testing-results.md`. Scores: core 78%, CLI 44%, server 92%
  (mostly collateral), Android unit tests 18%.
- The results identify concrete gaps (CLI PNG export, selection leases,
  `jmath` ports, Android UI logic) and show that one Rust test,
  `parity_snapshot`, catches most core mutants.
- Android scores do not cover the emulator tier. Covering it would need a far
  smaller sample or a faster emulator harness.
- `cargo mutants` output format changes can break `run_mutants.py`'s use of
  the `span`, `file`, `package` and `genre` fields. Update both together.
