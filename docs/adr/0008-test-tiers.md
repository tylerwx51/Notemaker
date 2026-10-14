# ADR 0008: Resource-specific automated test tiers

Status: accepted, 2026-10-13.

## Context

Most development must remain testable without the BOOX tablet or other computers.
Emulators validate application behavior, but cannot establish physical-device
compatibility or cross-machine connectivity. One-off installation checks also
need repeatable preservation gates and retained reports.

## Decision

`tests/run.py` provides four explicitly selected tiers. `local` is the default
and runs on one computer, including generated multi-proxy histories against
isolated real processes. `emulator` adds the existing Android functional and
performance suites. `hardware` requires a named ADB device; `network` requires
a configuration naming a main server and existing proxy endpoints. No external
device is automatically selected, and the tiers do not imply one another.

Each run records build/suite outcomes and logs, distinguishing pass, fail and
skip. Missing optional resources skip only their external checks;
`--require-resources` turns those skips into a failing result for lab automation.
An explicitly configured network that cannot converge fails. Suites run
sequentially so emulator ports and performance measurements do not conflict.
Existing individual test entry points remain supported.

Tablet smoke checks use the existing embedded proxy without changing documents.
Installation is separately requested. It stops the app, backs up the APK and
private data, verifies byte-for-byte data retention immediately after upgrade,
checks the APK hash, reconnects and verifies active-document actions and actor
histories. Backups are retained even on failure. Verification uses the JVM CLI
over a temporary ADB forward because standalone ART process shutdown can stall
on BOOX firmware. USB server mappings are changed only when explicitly supplied.

Network checks operate on already configured proxy trees, create uniquely named
test documents and verify durable final convergence. They never reconfigure or
kill external services. The same harness is regression-tested on an isolated
local proxy chain. Partitions, crashes and duplicate/lost messages continue to
be generated in the local topology suite from ADR 0007.

## Limits

Neither emulator timing nor a hardware smoke check proves E Ink quality or
physical pen latency. Live paid-provider checks remain opt-in. External test
documents are retained because there is no document deletion protocol. Private
tablet backups and endpoint tokens must remain outside version control. Keep
the selected tablet idle while checking exact actor-history preservation.

The separate opt-in `tests/tablet_drawing.py --serial SERIAL` check exercises
the installed editor using a uniquely named notebook and the embedded proxy.
It restores the original notebook and connection settings and verifies existing
actor histories afterward. Unlike the smoke tier, it creates test content;
the test notebook is retained. ADB forwarding is refreshed after instrumentation
restarts, since the embedded proxy publishes a new port for each process.
