# Bing Webmaster Tools connector (`bing-webmaster-tools`)

Use this connector to access **Bing Webmaster Tools** site metrics and diagnostics, and to submit
URLs for indexing.

This connector also includes **IndexNow** tools (optional) for fast URL submission via the
IndexNow protocol.

## Quick reference (LLM-friendly)

Bing Webmaster Tools API tools:

- `bing-webmaster-tools/list_sites`
- `bing-webmaster-tools/get_rank_and_traffic_stats`
- `bing-webmaster-tools/get_query_stats`
- `bing-webmaster-tools/get_query_traffic_stats`
- `bing-webmaster-tools/get_page_stats`
- `bing-webmaster-tools/get_crawl_stats`
- `bing-webmaster-tools/get_crawl_issues`
- `bing-webmaster-tools/get_keyword_data`
- `bing-webmaster-tools/get_backlinks`
- `bing-webmaster-tools/get_url_submission_quota`
- `bing-webmaster-tools/submit_url`
- `bing-webmaster-tools/submit_url_batch`
- `bing-webmaster-tools/get_url_info`
- `bing-webmaster-tools/get_deep_links`
- `bing-webmaster-tools/get_blocked_urls`
- `bing-webmaster-tools/get_query_page_stats`
- `bing-webmaster-tools/add_site`
- `bing-webmaster-tools/verify_site`
- `bing-webmaster-tools/get_content_issues`
- `bing-webmaster-tools/get_malware_issues`

IndexNow tools (optional):

- `bing-webmaster-tools/indexnow_submit_url`
- `bing-webmaster-tools/indexnow_submit_url_batch`

## Authentication

### Bing Webmaster Tools API key

Preferred (stores the key in rzn-tools config):

```bash
rzn-tools setup bing-webmaster-tools
```

Environment variable alternative:

- `BING_WEBMASTER_API_KEY`

To generate an API key:

1. Sign in to Bing Webmaster Tools: https://www.bing.com/webmasters/
2. Verify your site if you haven't already.
3. Go to **Settings → API Access** and generate an API key.

### IndexNow key (optional)

IndexNow uses a per-site key that must be **publicly hosted**:

- Default key file location: `https://<host>/<key>.txt`
- File contents: the key (plain text)

Configure it via setup (recommended):

```bash
rzn-tools setup bing-webmaster-tools
```

Or via environment variables:

- `INDEXNOW_KEY`
- Optional: `INDEXNOW_KEY_LOCATION` (only needed if your key file is not at the default URL)

Notes:

- IndexNow can be used even if you don't set a Bing API key, but all **Bing Webmaster Tools API**
  tools require `api_key`.
- rzn-tools defaults `key_location` to `https://<host>/<key>.txt` when omitted.

## MCP tool calling (arguments examples)

### `bing-webmaster-tools/get_query_stats`

```json
{
  "site_url": "https://example.com/",
  "response_format": "concise"
}
```

### `bing-webmaster-tools/submit_url_batch`

```json
{
  "site_url": "https://example.com/",
  "url_list": [
    "https://example.com/new-post",
    "https://example.com/updated-page"
  ]
}
```

### `bing-webmaster-tools/indexnow_submit_url`

```json
{
  "url": "https://example.com/new-post"
}
```

### `bing-webmaster-tools/indexnow_submit_url_batch`

```json
{
  "url_list": [
    "https://example.com/new-post",
    "https://example.com/updated-page"
  ]
}
```

## CLI examples

```bash
rzn-tools bing-webmaster-tools list-sites
```

```bash
rzn-tools bing-webmaster-tools get-query-stats --site-url https://example.com/
```

```bash
rzn-tools bing-webmaster-tools submit-url --site-url https://example.com/ --url https://example.com/new-post
```

```bash
rzn-tools bing-webmaster-tools indexnow-submit-url --url https://example.com/new-post
```

## Troubleshooting

- Authentication failures:
  - Ensure your Bing site is verified and your API key is active.
  - If using env vars, verify `BING_WEBMASTER_API_KEY` is set in the same shell.
- IndexNow failures:
  - Confirm your key file is accessible (HTTP 200) at `https://<host>/<key>.txt` (or your custom
    `key_location`).
  - Ensure all URLs in a batch share the same host unless you pass `host` explicitly.
