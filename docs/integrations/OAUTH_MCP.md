# OAuth via MCP Tools

Each connector exposes auth helpers as MCP tools. Downstream clients can render an “Authorize” button that calls `auth_start`, then poll with `auth_poll` until tokens are issued. By default, tokens are not persisted; set `RZN_PERSIST_TOKENS=1` to save them to `~/.config/rzn-tools/auth.json` for local dev.

Tool naming uses the MCP server’s prefixing: call tools as `connector/tool`.

Microsoft Graph
- Start: `microsoft-graph/auth_start` with args:
  - `tenant_id` (optional; default `common`)
  - `client_id` (required)
  - `scopes` (default: `offline_access Mail.Read Calendars.Read Files.Read`)
- Poll: `microsoft-graph/auth_poll` with args:
  - `tenant_id` (optional)
  - `client_id` (required)
  - `device_code` (required; from `auth_start`)

Google (Drive/Gmail/Calendar/People)
- Start: `google-drive/auth_start` with args:
  - `client_id` (required)
  - `scopes` (default: Drive read-only). For other Google connectors, pass combined scopes, e.g.:
    - Drive: `https://www.googleapis.com/auth/drive.readonly`
    - Gmail: `https://www.googleapis.com/auth/gmail.readonly`
    - Calendar: `https://www.googleapis.com/auth/calendar.readonly`
    - People: `https://www.googleapis.com/auth/contacts.readonly`
- Poll: `google-drive/auth_poll` with args:
  - `client_id` (required)
  - `client_secret` (optional for installed apps)
  - `device_code` (required)

Notes
- You can reuse the same Google tokens across connectors. The Drive connector saves to both `google-drive` and `google-common`. Gmail/Calendar/People look up `google-common` automatically.
- Device Authorization flow opens a verification URL and shows a `user_code`. A client can open the URL in a browser when the user clicks “Authorize”.
- Host-managed refresh: the server does not auto-refresh access tokens. If a call returns 401, hosts should refresh and call again.

Sample tool calls (JSON-RPC)
- List tools: `{"method":"tools/list","params":{}}`
- Start auth (Graph):
  - `{"method":"tools/call","params":{"name":"microsoft-graph/auth_start","arguments":{"client_id":"YOUR_ID"}}}`
- Poll (Graph):
  - `{"method":"tools/call","params":{"name":"microsoft-graph/auth_poll","arguments":{"client_id":"YOUR_ID","device_code":"..."}}}`
- Start auth (Google):
  - `{"method":"tools/call","params":{"name":"google-drive/auth_start","arguments":{"client_id":"YOUR_ID","scopes":"https://www.googleapis.com/auth/drive.readonly https://www.googleapis.com/auth/gmail.readonly"}}}`
- Poll (Google):
  - `{"method":"tools/call","params":{"name":"google-drive/auth_poll","arguments":{"client_id":"YOUR_ID","device_code":"..."}}}`
