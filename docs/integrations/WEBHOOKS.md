# Webhooks + MCP: How to Wire Them

MCP transports are request/response; they do not expose an HTTP endpoint. For providers that send push notifications (Google Drive/Calendar, Microsoft Graph), the host application must:

- Expose an HTTPS endpoint the SaaS can call.
- Verify/authenticate the webhook per provider docs.
- Persist subscription metadata (id/resourceId/expiration or channelId)
- Trigger follow‑up MCP tools that perform the actual sync (changes/list, events syncToken, Graph delta).

Recommended flow
- Create subscription (via admin tools like `google-drive/watch_files` or `google-calendar/watch_events`).
- Host receives webhook → minimal handler extracts IDs, headers, and tokens.
- Host calls the appropriate MCP tool:
  - Drive: `google-drive/list_changes` with the last stored `page_token`; update tokens and schedule renewal.
  - Calendar: `google-calendar/sync_events` with the last `syncToken`; store new token; on 410 GONE, do a full resync.
  - Graph: use delta endpoints (future tool) and renew subscriptions before `expiration`.
- Renew subscriptions well before expiration; store renewal schedule in the host.

Security
- Do not place verification secrets in the MCP server; keep them in the host store.
- Prefer short‑lived subscriptions; rotate tokens as needed.

Notes
- This repo hides webhook-setup tools from default `list_tools` unless `RZN_SHOW_ADMIN_TOOLS=1`.
- The LLM should usually call delta/sync tools, not the watch/subscribe tools.
