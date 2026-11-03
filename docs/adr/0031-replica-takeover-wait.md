# ADR 0031: Wait for a killed predecessor before owning the replica

Status: accepted, 2026-11-03.

## Context

The app's embedded replica proxy (`ReplicaService.ensure` →
`LocalProxy::new`) takes an exclusive `fcntl` lock on `files/proxy.lock` and
listens on `127.0.0.1:8766`. Both are released only when the owning process
exits.

Android often starts a replacement process right after killing the old one:
`am force-stop`, instrumentation restarts, and low-memory kills. The killed
process can still be exiting, especially on a loaded system. The replacement
then found the lock held ("A proxy already owns this replica") or the port
bound ("Address already in use"). `MainActivity.load` reported this as
`RuntimeException("Cannot read saved document")`, and the activity crashed on
launch. The Java implementation had the same single attempt. The emulator
suites hit it intermittently under the pre-commit hook's parallel load
(`cli_android` and `chat_ui`). Until commit `35d7024e`, the thrown exception
also dropped its cause, which hid the reason.

## Decision

`replica_service::ensure` retries `LocalProxy::new` every 50 ms for up to 5 s
while the failure is one of those two "held by another process" errors. Any
other error, or a holder that is still there after 5 s, fails as before.

The CLI's `serve` command is unchanged. A second proxy started by hand still
fails immediately.

## Consequences

- A relaunch that overlaps its predecessor's exit opens the replica once the
  lock and port are free, instead of crashing.
- If another process really owns the replica (for example a CLI `serve`
  under the app UID), app startup blocks for 5 s on the main thread before it
  crashes with the same error as before. That stays under Android's activity
  launch timeout.
- This deviates from the Java behavior, which failed on the first attempt.
