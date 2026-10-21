# ADR 0006: Durable server chats with replaceable agent harnesses

Status: accepted, 2026-10-13.

## Context

The sole user wants to start an autonomous note-editing agent from a BOOX popup,
resume global chat history, see text as it is generated, and inspect tool activity.
The development computer is the primary server on port 8765. Pi must start in
this repository with read/write access, use GPT-6-luna with medium effort by
default, and use the existing CLI skill without edit approvals.

## Decision

Add separate chat, message-request, and event tables to the server SQLite
database. Document operations and the shared local replica protocol remain the
only supported note-editing path. Each chat has a stable `agent-<chat UUID>` actor;
Pi uses a shared server-side CLI replica and synchronizes through the HTTP server.
The harness supplies `NOTEMAKER_ACTOR` and `NOTEMAKER_STATE` as CLI defaults so
the model does not have to transcribe UUIDs or replica paths. Explicit `--actor`
continues to override the environment for ordinary CLI callers.
The prompt supplies routing context (current document, server document names,
upstream and replica location), leaving content discovery to the CLI and skill.
As of 2026-10-21, Android also attaches its current app launch session ID to
each message. Pi receives it as `app_session_id`, so it can address the live
tablet through CLI UI commands. Older clients and saved retries without an ID
remain accepted.

A harness interface has two implementations: deterministic non-editing `mock`
and installed `pi`. Harness, provider/model and thinking effort are selected at
chat creation and fixed for that conversation; new chats can select different
settings. The default provider is `openai` for both Luna models (updated at the
user's request from `openai-codex`). The server model allowlist is configurable. Pi resumes one native
session file per chat. There is no multi-agent orchestration.

Pi runs in Bubblewrap with a writable checkout and agent-data directory, a
private temporary home, isolated process namespace, read-only Nix runtime and
installed Pi package, and shared networking. Its dedicated config starts with
copies of the host's Pi auth and model files and can refresh OAuth credentials.
Other home files are not mounted. The current writable-checkout policy is
intentional and can be tightened later. Network access includes the upstream
server and model provider; this is not a network allowlist. No unsandboxed
fallback is permitted. Runtime dependencies come from `shell.nix`; the existing
Pi installation is selected with `--pi-command` rather than downloaded at startup.

An explicitly loaded Pi extension uses `message_update`, `message_end`, and tool
execution hooks to emit normalized text and tool events. It appends framed JSON
to a private per-run file which the server tails every 25 ms and removes after
draining. This avoids interleaving with Pi's own buffered RPC output and handles
large tool results without nonblocking-pipe write failures.
Thinking content is not included in the tablet transcript. The worker waits for
`agent_settled`, not `agent_end`, so retries and compaction remain part of the run.
Tool display content is capped at 32,000 characters; Pi retains its native session.

Every display event is journaled before delivery over a reconnectable NDJSON
stream with monotonically increasing cursors. The Android popup groups redraws
at 250 ms, updates changed rows, and leaves tool details collapsed until tapped.
The popup's transparent window handles system/keyboard insets explicitly, sizing
the visible panel to the remaining area so the composer and Send button stay
above the keyboard. Tool summaries include filenames or commands; expanded
arguments use labels and verbatim code rather than compact JSON.
Closing the popup ends only the subscription. The server continues running and
history can be replayed. Recent chats are global, ordered by latest activity;
the hub shows five and More chats opens paginated history. Local drafts and
uncertain send IDs survive reopening.

Client-generated chat and message IDs make retries idempotent. Only one run per
chat and four runs across the server are accepted at once. Stop terminates the
sandbox process group; already committed edits are retained. A server restart
marks unfinished runs interrupted rather than automatically retrying potentially
completed edits. Runs have a 30-minute limit.

## Consequences

The existing trusted-LAN server now launches an agent with repository write
access. Deploy only within that established trust boundary. Chat mutations
require JSON and reject cross-origin browser requests; this is not user
authentication. Credentials and session files live in ignored private agent data.
Back up the SQLite database and agent data together to retain transcripts and Pi
continuations. A server crash cannot atomically reconcile the chat event journal,
Pi's native transcript, and document edits; interrupted runs require review.

Offline document editing is unchanged. New agent work needs a reachable server;
unsent drafts stay on the tablet, and sends are retried explicitly. Complete chat
history is stored on the server rather than cached for offline browsing.
Physical E Ink refresh and BOOX keyboard behavior still need device validation.
