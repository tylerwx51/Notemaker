---
name: notemaker-server
description: Work with the NoteMaker tablet sync server through its CLI to inspect documents, edit notes or diagrams, synchronize replicas, and troubleshoot connectivity. Use for NoteMaker server workflows.
---

# NoteMaker server

Work from this repository's root. Read its `AGENTS.md` for current operating context.
Run commands inside the repository's `nix-shell`. Build with `apps/cli/build.sh`
if needed, then **read `apps/cli/notemaker --help` before using the CLI**.
Read `apps/cli/README.md` for JSON schemas and the root `README.md` for server
startup, proxy access, and tablet connections; keep the server URL configurable.

Use the selected running `--proxy-file` descriptor (or `NOTEMAKER_PROXY_FILE`), an
explicit `--document`, and a stable, distinct `--actor` for edits. All editing
goes through the selected proxy and works with its upstream offline; never mutate the server or
its database directly. Keep whole diagrams atomic. Commands return JSON;
check exit status and inspect `log` before retrying an uncertain mutation.
`sync [UPSTREAM]` requests synchronization; the proxy also synchronizes automatically.
Direct `--state` access is read-only. Start a proxy with `--state DIR serve PORT [UPSTREAM]`
if needed. Never bypass an unavailable selected proxy by writing its files.
`document list` only lists local documents. Do not infer remote deployment from
a request to work locally.

Examples (inside `nix-shell`; substitute the intended replica, document and URL):

```sh
# Inspect the local replica's sync status.
apps/cli/notemaker --state /tmp/notemaker-example --document demo status

# Add one atomic diagram locally, then synchronize it.
apps/cli/notemaker --proxy-file /tmp/notemaker-example/proxy.json --document demo --actor agent-1 diagram add apps/cli/examples/diagram.json
apps/cli/notemaker --proxy-file /tmp/notemaker-example/proxy.json --document demo sync http://127.0.0.1:8765
```

Use a fresh example replica/document: the diagram file has fixed IDs and cannot
be added twice to the same document.
