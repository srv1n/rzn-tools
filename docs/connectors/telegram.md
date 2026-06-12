# Telegram — Connector Notes

Status: Implemented (MVP, MTProto via `grammers`)

## Overview

This connector integrates with Telegram using the official MTProto protocol via the Rust
`grammers-*` crates (license: MIT OR Apache-2.0). It supports reading dialogs and messages and
sending messages once you have a local session.

## TL;DR Setup (copy/paste)

Telegram MTProto clients require a **developer API ID** and **API hash**:

1. Create an app at https://my.telegram.org/apps
2. Set environment variables (recommended; works well with MCP clients like Claude Desktop):

```bash
export TG_ID="123456"
export TG_HASH="0123456789abcdef0123456789abcdef"
export TG_SESSION_FILE="$HOME/.config/rzn-tools/telegram.session"  # optional
```

3. Start the MCP server (example: build/run from source):

```bash
cargo run -p rzn_tools_mcp --bin rzn-tools-mcp --features full  # or: --features telegram
```

4. In your MCP client, call:
   - `telegram/start_login` with `{ "phone": "+15551234567" }`
   - `telegram/complete_login` with `{ "code": "12345", "password": "..." }`

Note: `start_login` stores a short-lived pending token in memory, so `complete_login` must be
called on the same running rzn-tools/MCP process (if you restart, just call `start_login` again).

## Configuration

rzn-tools reads Telegram settings from either connector auth config or environment variables.

**Auth fields (connector config):**
- `api_id` (required) – Telegram developer API ID
- `api_hash` (required) – Telegram developer API hash
- `session_file` (optional) – path to the `.session` file

**Environment variables:**
- `TG_ID` – same as `api_id`
- `TG_HASH` – same as `api_hash`
- `TG_SESSION_FILE` – same as `session_file`

**Default session file path:**
- `~/.config/rzn-tools/telegram.session`

Treat the session file as a secret (it can grant access to the account).

## Tools

All tools are exposed under `telegram/*`:

- `telegram/status`: session authorized? (best-effort includes `me`)
- `telegram/start_login`: send login code to phone
- `telegram/complete_login`: complete login with code (+ optional 2FA password)
- `telegram/resolve_username`: resolve `@username` → `peer_ref`
- `telegram/list_dialogs`: list dialogs with `peer_ref` objects
- `telegram/get_messages`: fetch recent messages for a `peer_ref`
- `telegram/search_messages`: search messages in a dialog
- `telegram/send_message`: send a message to a dialog

### Peer References

Most read/write tools accept a `peer_ref`:

```json
{
  "id": -1001234567890,
  "access_hash": 987654321012345678
}
```

You can obtain these from `telegram/list_dialogs` or `telegram/resolve_username`.

## Notes / Limitations

- This connector accesses personal chats if you authorize a user account.
- Telegram flood-wait errors can occur if you do heavy history scraping; keep limits small.
- If your account is brand new, Telegram may require you to sign up in the official client first
  (you may see a `sign-up required` error).

## Troubleshooting

**`Missing Telegram api_id/api_hash`**
- Ensure `TG_ID`/`TG_HASH` are set in the environment of the running MCP server process.

**`session not authorized`**
- Run `telegram/start_login`, then `telegram/complete_login` (same server process).

**2FA**
- If you enabled Telegram 2FA, pass `password` to `telegram/complete_login`.
