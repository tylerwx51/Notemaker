---
name: notemaker-server
description: Work with the NoteMaker tablet sync server through its CLI to inspect documents, edit notes or diagrams, synchronize replicas, and troubleshoot connectivity. Use for NoteMaker server workflows.
---

# NoteMaker server

Work from this repository's root. Read its `AGENTS.md` and
`../computer-it/NETWORK_OPERATING_GUIDE.md` for current operating context.
Run commands inside the repository's `nix-shell`. Build with `apps/cli/build.sh`
if needed, then **read `apps/cli/notemaker --help` before using the CLI**.
Read `apps/cli/README.md` for JSON schemas and the root `README.md` for server
startup, proxy access, and tablet connections; keep the server URL configurable.

Use the intended local `--state` directory or `--proxy-file` descriptor, an
explicit `--document`, and a stable, distinct `--actor` for edits. All editing
goes through shared local state and works offline; never mutate the server or
its database directly. Keep whole diagrams atomic. Commands return JSON;
check exit status and inspect `log` before retrying an uncertain mutation.
`sync URL` uploads and downloads; repeat while `pending` is nonzero.
`document list` only lists local documents. Do not infer remote deployment from
a request to work locally.

Examples (inside `nix-shell`; substitute the intended replica, document and URL):

```sh
# Inspect the local replica's sync status.
apps/cli/notemaker --state /tmp/notemaker-example --document demo status

# Add one atomic diagram locally, then synchronize it.
apps/cli/notemaker --state /tmp/notemaker-example --document demo --actor agent-1 diagram add apps/cli/examples/diagram.json
apps/cli/notemaker --state /tmp/notemaker-example --document demo sync http://127.0.0.1:8765
```

Use a fresh example replica/document: the diagram file has fixed IDs and cannot
be added twice to the same document.
