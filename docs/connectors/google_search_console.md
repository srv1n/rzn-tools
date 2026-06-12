# Google Search Console connector (`google-search-console`)

Use this connector to pull **real Search Console data** (clicks, impressions, CTR, position),
manage **sitemaps**, and run **URL Inspection** for index/debugging workflows.

If you already have Google Workspace connectors configured (Drive/Gmail/Calendar), note that
Search Console uses a *different* scope set and is usually configured separately.

## Quick reference (LLM-friendly)

Tools:

- `google-search-console/list_sites` — list properties (sites)
- `google-search-console/get_site` — get property details + permission level
- `google-search-console/search_analytics` — clicks/impressions/CTR/position by query/page/country/device/date
- `google-search-console/list_sitemaps` — list submitted sitemaps
- `google-search-console/get_sitemap` — sitemap details (errors/warnings/last submitted)
- `google-search-console/submit_sitemap` — submit a sitemap URL
- `google-search-console/delete_sitemap` — remove a sitemap URL
- `google-search-console/inspect_url` — URL Inspection API (index/crawl/rich results)
- `google-search-console/query_builder` — preset query args for `search_analytics`

Defaults:

- Most tools accept `response_format=concise|detailed` (use `detailed` for raw Google payloads).

## Authentication (recommended)

Run the interactive setup (stores tokens in rzn-tools config):

```bash
rzn-tools setup google-search-console
```

rzn-tools uses the **Google OAuth device flow**, but you must provide your own OAuth client credentials.

### Step-by-step (Google Cloud Console)

1. Verify your site/property in Google Search Console (so your account has access).
2. Create a Google Cloud project.
3. Enable the **Google Search Console API** in your project.
4. Create an **OAuth Client ID** (Desktop app recommended).
5. Run `rzn-tools setup google-search-console` and paste:
   - `client_id` (required)
   - `client_secret` (optional; depends on client type)
6. Complete the device flow in the browser when prompted.

### Scope notes

rzn-tools uses the following scope for this connector:

- `https://www.googleapis.com/auth/webmasters`

This scope is required for **URL Inspection**. If you only need read-only analytics, you can
create a separate client with `webmasters.readonly`, but `inspect_url` may fail with
insufficient permissions.

## Property URL formats (`site_url`)

Google Search Console uses two common formats:

- Domain property: `sc-domain:example.com`
- URL-prefix property: `https://example.com/` (note trailing slash is common)

Pass exactly what Search Console uses for the property.

## MCP tool calling (arguments examples)

### `google-search-console/search_analytics`

Top queries for the last 28 days:

```json
{
  "site_url": "sc-domain:example.com",
  "start_date": "2026-02-05",
  "end_date": "2026-03-04",
  "dimensions": "query",
  "row_limit": 1000,
  "data_state": "final"
}
```

Filter queries containing a keyword:

```json
{
  "site_url": "sc-domain:example.com",
  "start_date": "2026-02-05",
  "end_date": "2026-03-04",
  "dimensions": ["query"],
  "dimension_filter_groups": [
    {
      "filters": [
        { "dimension": "query", "operator": "contains", "expression": "pricing" }
      ]
    }
  ]
}
```

### `google-search-console/inspect_url`

```json
{
  "site_url": "sc-domain:example.com",
  "inspection_url": "https://example.com/blog/post",
  "language_code": "en-US"
}
```

## CLI examples

```bash
rzn-tools google-search-console list-sites
```

```bash
rzn-tools google-search-console search-analytics \
  --site-url sc-domain:example.com \
  --start-date 2026-02-05 \
  --end-date 2026-03-04 \
  --dimensions query \
  --row-limit 1000
```

```bash
rzn-tools google-search-console inspect-url \
  --site-url sc-domain:example.com \
  --inspection-url https://example.com/blog/post
```

## Troubleshooting

- `403` / insufficient permissions:
  - Confirm the Google account you authorized has access to the property in Search Console.
  - Confirm you requested the `webmasters` scope if you are calling `inspect_url`.
- Empty results:
  - Use `response_format=detailed` to inspect the raw payload and verify query params.
  - Check `data_state` (`final` vs `all`) depending on freshness needs.
