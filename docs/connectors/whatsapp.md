# WhatsApp — Connector Notes

Status: Implemented (MVP, WuzAPI sidecar)

## Overview

This connector integrates with WhatsApp through a local WuzAPI sidecar (Go) which wraps the
`whatsmeow` library.

**Important:** This is an unofficial WhatsApp multi-device client. Using unofficial clients may
violate WhatsApp ToS and can get your number banned. Avoid spam and use at your own risk.

## TL;DR Setup (recommended)

1. Install and run WuzAPI locally
2. Create (or reuse) a WuzAPI *user token*
3. Configure rzn-tools with `WUZAPI_BASE_URL` + `WUZAPI_TOKEN`
4. Call `whatsapp/connect` and scan the QR code

## Step-by-step Setup

### 1) Install WuzAPI

WuzAPI is a separate binary. On macOS/Linux you can use Homebrew:

```bash
brew install asternic/wuzapi/wuzapi
```

Or build from source (requires Go):

```bash
git clone https://github.com/asternic/wuzapi.git
cd wuzapi
go build .
./wuzapi -version
```

### 2) Configure and run WuzAPI (HTTP mode)

Pick a directory to store WuzAPI state (SQLite + session files):

```bash
mkdir -p "$HOME/.config/rzn-tools/wuzapi"
```

WuzAPI supports a `.env` file. Create one in the directory you will run `wuzapi` from:

```bash
cat > .env <<'EOF'
# Required for admin endpoints (user management)
WUZAPI_ADMIN_TOKEN=change_me_32_chars

# Recommended: persist encryption key so restarts keep encrypted data readable
# (if omitted, WuzAPI may auto-generate it)
WUZAPI_GLOBAL_ENCRYPTION_KEY=change_me_32_chars
EOF
```

If you omit these, WuzAPI will generate random values and print them in logs. Copy them into your
`.env` file to avoid losing encrypted data or lock yourself out on restart.

Then start WuzAPI:

```bash
wuzapi -address 127.0.0.1 -port 8080 -datadir "$HOME/.config/rzn-tools/wuzapi"
```

You should be able to hit:
- `GET http://127.0.0.1:8080/health`

### 3) Create (or reuse) a WuzAPI user token

rzn-tools uses the **user token** for normal endpoints (`/session/*`, `/chat/*`, `/group/*`, etc.).

You can list existing users and copy a token:

```bash
export WUZAPI_ADMIN_TOKEN="change_me_32_chars"
curl -s -H "Authorization: $WUZAPI_ADMIN_TOKEN" http://127.0.0.1:8080/admin/users
```

Or create a new user:

```bash
export WUZAPI_ADMIN_TOKEN="change_me_32_chars"
export WUZAPI_TOKEN="rzn_tools_user_token_change_me"

curl -s -X POST \
  -H "Authorization: $WUZAPI_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  --data "{\"name\":\"rzn-tools\",\"token\":\"$WUZAPI_TOKEN\",\"events\":\"Message\"}" \
  http://127.0.0.1:8080/admin/users
```

### 4) Configure rzn-tools

Set these in the environment of the running MCP server (recommended):

```bash
export WUZAPI_BASE_URL="http://127.0.0.1:8080"
export WUZAPI_TOKEN="rzn_tools_user_token_change_me"
```

Then run the MCP server (from source):

```bash
cargo run -p rzn_tools_mcp --bin rzn-tools-mcp --features full  # or: --features whatsapp
```

If you are configuring an MCP client (e.g. Claude Desktop), place the env vars in the MCP server
`env` block (see the top-level `README.md`).

### 5) Link your WhatsApp account (QR login)

In your MCP client, call:
- `whatsapp/health` (sanity check)
- `whatsapp/connect`

If the session is not logged in yet, `whatsapp/connect` returns `qr_code` as a
`data:image/png;base64,...` string. Open it in a browser or paste it into any QR viewer and scan
it from WhatsApp mobile:

WhatsApp → Settings → Linked devices → Link a device

### 6) Verify and send a message

Call:
- `whatsapp/status`

Send text:

```json
{
  "to": "15551234567",
  "message": "hello from rzn-tools"
}
```

Send media from a local file:

```json
{
  "to": "15551234567",
  "file_path": "/path/to/photo.jpg",
  "media_type": "image",
  "caption": "hi"
}
```

## Configuration Reference

rzn-tools reads WhatsApp settings from connector auth config or environment variables.

**Environment variables:**
- `WUZAPI_BASE_URL` (default `http://localhost:8080`)
- `WUZAPI_TOKEN` (required; WuzAPI user token)

**Optional (only if you want rzn-tools to auto-start WuzAPI):**
- `WUZAPI_PATH` (path to the `wuzapi` binary)
- `WUZAPI_ADDRESS` (default `127.0.0.1` when rzn-tools starts it)
- `WUZAPI_PORT` (default `8080` when rzn-tools starts it)
- `WUZAPI_DATA_DIR` (defaults to `~/.config/rzn-tools/wuzapi`)
- `WUZAPI_ADMIN_TOKEN` (passed as `-admintoken` when rzn-tools starts WuzAPI)

**Connector auth fields (equivalent to env vars):**
- `base_url` (`WUZAPI_BASE_URL`)
- `token` (`WUZAPI_TOKEN`)
- `wuzapi_path` (`WUZAPI_PATH`)
- `wuzapi_address` (`WUZAPI_ADDRESS`)
- `wuzapi_port` (`WUZAPI_PORT`)
- `data_dir` (`WUZAPI_DATA_DIR`)
- `admin_token` (`WUZAPI_ADMIN_TOKEN`)

## Notes / Limitations

- This connector does not currently manage WuzAPI users for you; create a user token in WuzAPI and
  set `WUZAPI_TOKEN`.
- Real-time incoming messages (webhooks) are not wired into rzn-tools yet; WuzAPI supports webhooks if
  you want to configure them directly.
- `whatsapp/get_messages` reads local chat history (requires history enabled in WuzAPI).

## Troubleshooting

**`WuzAPI user token not configured`**
- Set `WUZAPI_TOKEN` (this is the user token from WuzAPI `/admin/users`), not the admin token.

**401/403 from WuzAPI**
- Token mismatch. Verify the token you configured in rzn-tools matches the user token in WuzAPI.

**`Timed out waiting for WuzAPI /health`**
- WuzAPI is not running or `WUZAPI_BASE_URL` points to the wrong host/port.

**QR never appears**
- Call `whatsapp/status` to see `LoggedIn`/`Connected`.
- If `Connected=false`, call `whatsapp/connect` again.
