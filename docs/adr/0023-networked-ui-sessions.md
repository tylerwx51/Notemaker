# ADR 0023: Route live UI sessions through the proxy tree

Status: accepted, 2026-10-21.

## Context

[ADR 0022](0022-live-app-sessions.md) registered live app sessions only on the
tablet's local proxy. A CLI connected to another proxy or the main server could
see documents but could not identify or control the active tablet screen. The
tablet proxy binds to loopback and is often behind USB or another network
boundary, so the server cannot open an inbound connection to it.

## Decision

Each proxy with a live app session sends its state to its configured upstream
every second. Intermediate proxies forward the heartbeat to their upstream;
the main server keeps an in-memory registry with a 12-second expiry. Each record
includes a generated device ID, device name, app session ID, selected document,
page, layer, tool, foreground status, session start, proxy ID, and server last
seen/expiry times. The registry is transient and never enters document history.
An offline proxy can serve the subset of sessions reachable below it.

`ui sessions` queries the reachable root, and `ui state`, `ui actions`, and
`ui action` route through the same proxy tree. The root queues a command for the
owning proxy. The tablet's outbound heartbeat picks it up and returns a result
through the tree. Commands carry a request ID; the tablet proxy executes each
ID once and repeats its result until the root acknowledges it. A timed-out
command reports uncertainty so callers inspect the session before retrying.
Presence and pending commands expire; a server restart creates an empty registry
that clients repopulate automatically.

The proxy's existing bearer token protects its CLI `/command` endpoint. As with
the existing document server API, root UI endpoints rely on the configured
trusted network; do not expose the main server to an untrusted network.

## Verification

Build the CLI and APK in the root Nix shell. Run `tests/ui_topology_android.py`
for a real Android app, two proxy hops, server enumeration, remote document
navigation, and expiry after the app closes. Also run the existing local UI,
server, and proxy regression suites. Physical tablet routing needs the updated
server and every proxy on its path.
