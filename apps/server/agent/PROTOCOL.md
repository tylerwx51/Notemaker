# Agent chat protocol

Chat endpoints share the configured NoteMaker HTTP server. All POSTs require
`Content-Type: application/json`. Errors are JSON `{ "error": "..." }`; conflict
is 409, missing chat is 404. Model/provider/harness arguments are validated against
the server options, never interpreted as shell command text.

| Endpoint | Result |
| --- | --- |
| `GET /api/chats/options` | `harnesses`, `models` with `provider`, `model`, `efforts`, and `defaults` |
| `GET /api/chats?limit=5&offset=0` | `{chats: [...], more: boolean}`; global latest activity first; max limit 100 |
| `POST /api/chats` | Create with `{id: canonical UUID, harness, provider, model, effort}`; return chat |
| `GET /api/chats/ID` | Chat metadata including config, title, status, updated Unix timestamp |
| `POST /api/chats/ID/messages` | `{request: unique ID, text, document, session?: app session ID}` → `{accepted: ID}` |
| `POST /api/chats/ID/stop` | `{}` → `{stopping: boolean}` |
| `GET /api/chats/ID/events?after=0` | `{events: [{seq, event}, ...]}`, at most 500; continue after last seq |
| `GET /api/chats/ID/stream?after=0` | NDJSON `{seq,event}` records flushed as generated; `{}` heartbeat; reconnect after 20 seconds |

Creation and message submission are idempotent for the same ID and body.
Reusing an ID with different content is rejected. Retrying an accepted message
never starts it again, including after restart. Only one active message per chat
is accepted. Text is limited to 32,000 characters. Configuration is fixed per
chat; choose another harness/model when creating another chat. Android includes
its current launch's UI session ID in `session`; older clients and saved retries
may omit it. The Pi prompt receives it as `app_session_id` alongside the current
document and server document names. The user event records it when present.

Event `kind` values:

- `user`: `id`, `text`, `document`, and optional `session`.
- `assistant`: `id`, initial `text`.
- `delta`: append `text` to the assistant row identified by `id`.
- `assistant_end`: replace the row text with authoritative `text`.
- `tool`: `id`, `name`, JSON-string `args`, initial `text`, `status`.
- `tool_update`: replace tool `text` for `id` with the latest partial output.
- `tool_end`: replace tool `text` and `status` (`done` or `error`).
- `error`: visible provider error `text`; a retry can still follow.
- `status`: `status` and explanatory `text`; `running`, `done`, `error`, `stopped`,
  or `interrupted`. An idle chat has no message events yet.

Sequence numbers are global across chats and can have gaps. Deduplicate using
`seq` and resume from the last applied sequence. Tool IDs correlate expandable
activity; assistant IDs distinguish successive responses within a run. Closing
a stream does not cancel the run. `stopped` preserves committed edits;
`interrupted` means the server restarted and the request must not be replayed
automatically. Displayed tool arguments/results are capped at 32,000 characters.
