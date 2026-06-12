# Slack — Design Spec

Status: Draft (Phase 1)

## Overview

Read and search Slack content (channels, DMs, threads, files, bookmarks) to power RAG and summarization. Start read‑only; add write tools later (posting summaries).

## Key Use Cases

- Fetch a full discussion thread by URL for context.
- Search recent messages across channels the user can access.
- List unread or recently active channels/DMs for triage.
- Retrieve files shared in a thread with metadata.

## MVP Scope (Tools)

- `list_channels`: return channels/DMs the token can access.
- `list_messages`: list recent messages in a channel with optional `thread_ts`.
- `get_thread`: fetch root + replies given a `channel` and `thread_ts`.
- `search_messages`: Slack search with query string; return matched messages + context.
- `list_files`: files by channel or by user/time window.
- `test_auth`: verify token and team info.
- `get_thread_by_permalink`: convenience tool that parses a Slack message permalink (archives/ or app.slack.com forms) and fetches the thread. If the permalink points to a reply, uses `thread_ts` when present; otherwise assumes the linked message is the root.

Inputs follow our JSON Schema pattern; outputs include structured items with `text` for summaries and `provenance` (`team_id`, `channel_id`, `ts`, `permalink`).

## API & Auth

- Auth: Slack OAuth 2.0 (user or bot token). Start with bot token for server deployments.
- Scopes (read‑only MVP): `channels:read`, `groups:read`, `im:read`, `mpim:read`, `channels:history`, `groups:history`, `im:history`, `mpim:history`, `users:read`, `files:read`.
- Endpoints (Web API): `conversations.list`, `conversations.history`, `conversations.replies`, `search.messages`, `files.list`, `users.info`.
- Pagination: cursor‑based (`response_metadata.next_cursor`).
- Rate limits: handle 429 with `Retry-After`; propagate `x-slack-req-id` in logs.

## Rust Crates / Deps

- HTTP: `reqwest` + `serde` (preferred for control and stability).
- Optional SDK: evaluate `slack-morphism`. If adopted, wrap in a thin adapter to our traits.
- Common: `tokio`, `thiserror`, `url`, `mime_guess`.

## Data Model

- `SlackMessage` (channel_id, ts, user, text, blocks as raw JSON, files[], thread_ts, reactions[], permalink).
- `SlackFile` (id, name, mimetype, size, url_private, thumb, created_by, channels[]).
- ACL: include `team_id`, `channel_id`, and `is_private` flag for downstream enforcement.

## Error Handling & Limits

- Map Slack errors to `ConnectorError::Other { code, message }`.
- Backoff on 429; cap page sizes; guard file downloads by size/MIME.

## Security & Privacy

- Never log message bodies/file names in info logs; redact in traces.
- Honor channel privacy; do not surface private channels outside the token’s scope.

## Local vs Server

- Server: full Web API integration.
- Desktop helpers: open deep links (`slack://channel?...`) and quick navigation tools (non‑API).

## Testing Plan

- Record fixtures for `conversations.*` and `search.messages` with sanitized data.
- Acceptance: fetch known thread by permalink and reconstruct ordered conversation with files.

## Implementation Checklist

- [ ] OAuth + token storage; `test_auth`
- [ ] `list_channels`, `list_messages`, `get_thread`, `search_messages`, `list_files`
- [ ] Pagination and 429 backoff
- [ ] File metadata (no content download in MVP)
- [ ] Docs and examples

## Quick Start (build + configure)

- Build CLI with Slack enabled: `cargo build -p rzn_tools_cli --features slack`
- Set token: `rzn-tools config set slack token xoxb-...` (bot or user token with read scopes)
- Test: `rzn-tools config test slack` or call `rzn-tools tools slack`

### Examples

```bash
# List channels
rzn-tools slack channels --limit 100

# Get channel messages
rzn-tools slack messages --channel general --limit 50

# Search messages
rzn-tools slack search --query "project update" --limit 20

# List users
rzn-tools slack users --limit 100

# Use --help for all options
rzn-tools slack --help
```
