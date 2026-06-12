# LLM-Focused Tooling

This document summarizes which tools are exposed by each connector, the default output shapes optimized for LLMs, and how to enable optional composite "macro" tools.

By default, surfaces favor small, predictable responses (concise) and hide admin/setup endpoints to keep the action space simple for agents. Use `response_format: "detailed"` when you need the full provider payloads.

For downstream **ingestion/indexing** (building corpora, card indexes, etc.), rzn-tools tools can also expose an opt-in normalized output format (`output_format: "normalized_v1"`) designed to be chunk-ready and pagination-friendly. See `docs/integrations/INGEST_CONTRACT_V1.md`.

## Defaults LLMs Should Use

- Concise responses by default:
  - Use `response_format` to switch to `detailed` when needed.
  - Pagination tokens returned in concise where applicable (e.g., Drive `nextPageToken`, Gmail `nextPageToken`, Calendar `nextPageToken`/`nextSyncToken`, Graph `@odata.nextLink`).
- Admin/watch/subscribe tools are hidden by default to reduce surface area:
  - Set `RZN_SHOW_ADMIN_TOOLS=1` to show watch/subscription tools in `list_tools`.
- OAuth and token refresh:
  - If a connector has a `refresh_token` stored in rzn-tools config, rzn-tools can refresh access tokens as needed.
  - If only an `access_token` is present, you may need to re-run `rzn-tools setup <connector>` after expiry.
  - Dev-only persistence toggles may exist (e.g., `RZN_PERSIST_TOKENS=1`) depending on deployment mode.

## Connector Tool Surfaces (LLM-facing)

### Microsoft Graph (`microsoft-graph`)
- list_messages (concise|detailed): `{ id, subject, from, receivedDateTime }`, `nextLink` via `@odata.nextLink`
- get_message
- list_events (concise|detailed): `{ id, subject, start, end }`, `nextLink`
- send_mail, create_draft, upload_attachment_large, upload_attachment_large_from_path, send_draft
- auth_start, auth_poll (device code)

Optional macro (feature: `llm-macros`):
- send_with_attachments: Compose draft, attach (base64 or file_path), send.
  - Input: `{ to: string[], subject: string, body_text: string, attachments?: [{ filename, mime_type, data_base64? , file_path? }] }`
  - Output: `{ status: "sent", message_id }`

### Google Drive (`google-drive`)
- list_files (concise|detailed): `{ id, name, mime_type, size, modified_time }`, `nextPageToken`
- get_file (concise|detailed)
- download_file: returns `{ name, mime_type, size, data_base64 }`
- export_file: Google Docs/Sheets/Slides → `{ filename, data_base64 }` (extension inferred from target MIME)
- upload_file, upload_file_resumable
- auth_start, auth_poll (device code)

Admin-only (hidden unless `RZN_SHOW_ADMIN_TOOLS=1`): watch_files, watch_file, list_changes, get_start_page_token, stop_channel, upload_file_from_path

Optional macro (feature: `llm-macros`):
- find_and_export: Search by Drive query or export a known `file_id`; returns exported content.
  - Input: `{ q?: string, file_id?: string, target_mime: string }`
  - Output: `{ source_file_id, source_mime, export_mime_type, filename, data_base64 }`

### Gmail (`google-gmail`)
- list_messages (concise|detailed): `{ id, threadId }`, `nextPageToken`
- get_message (format: raw|full|metadata)
- get_thread
- decode_message_raw: base64url → MIME parts (headers, bodies, attachments)

### Google Calendar (`google-calendar`)
- list_events (concise|detailed): `{ id, summary, start, end }`, `nextPageToken`, `nextSyncToken`
- create_event, update_event, delete_event
- sync_events (syncToken)

Admin-only: watch_events, stop_channel

### Google People (`google-people`)
- list_connections (concise|detailed): `{ resourceName, name, email }`, `nextPageToken`
- get_person (concise|detailed)

### Google Search Console (`google-search-console`)
- list_sites (concise|detailed): properties and permission levels
- get_site (concise|detailed)
- search_analytics (concise|detailed): `{ rows, response_aggregation_type }` (dimensions + filters)
- list_sitemaps, get_sitemap, submit_sitemap, delete_sitemap
- inspect_url (concise|detailed): URL Inspection API
- query_builder: preset arguments for common `search_analytics` patterns

Notes:
- Property IDs look like `sc-domain:example.com` (domain) or `https://example.com/` (URL prefix).
- URL inspection requires the `https://www.googleapis.com/auth/webmasters` scope.

### Bing Webmaster Tools (`bing-webmaster-tools`)
- list_sites
- get_rank_and_traffic_stats, get_query_stats, get_page_stats
- get_crawl_stats, get_crawl_issues
- get_keyword_data, get_backlinks
- get_url_submission_quota, submit_url, submit_url_batch
- get_url_info, get_deep_links, get_blocked_urls, get_query_page_stats
- add_site, verify_site, get_content_issues, get_malware_issues

IndexNow (optional, separate key):
- indexnow_submit_url, indexnow_submit_url_batch

## Enabling Macro Tools

Macro tools bundle multiple steps into one call and are disabled by default.

Enable them by building with the `llm-macros` feature on the core crate. The CLI forwards this feature, so you can toggle it on the CLI target as well.

Examples:

```bash
# Build CLI with productivity connectors only
cargo build -p rzn_tools_cli \
  --features "microsoft-graph,google-drive,google-gmail,google-calendar,google-people"

# Build with macros enabled (composite tools)
cargo build -p rzn_tools_cli \
  --features "microsoft-graph,google-drive,google-gmail,google-calendar,google-people,llm-macros"
```

JSON-RPC examples:

```json
// google-drive/find_and_export
{"method":"tools/call","params":{"name":"google-drive/find_and_export","arguments":{"q":"name contains 'PRD' and mimeType='application/vnd.google-apps.document'","target_mime":"application/pdf"}}}

// microsoft-graph/send_with_attachments
{"method":"tools/call","params":{"name":"microsoft-graph/send_with_attachments","arguments":{"to":["a@example.com"],"subject":"Hi","body_text":"Please see attached.","attachments":[{"filename":"report.pdf","mime_type":"application/pdf","data_base64":"..."}]}}}
```

## Admin Tools and Webhooks

Admin-only tools for watch/subscribe and changes are hidden by default. For webhook design and renewal strategy, see `docs/integrations/WEBHOOKS.md`.
