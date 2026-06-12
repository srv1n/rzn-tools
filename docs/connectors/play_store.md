# Play Store Connector (`play-store`)

The `play-store` connector fetches public Google Play listing metadata for an Android package ID
using **best-effort HTML parsing** (no reverse-engineered internal APIs; no auth required).

## Tools

### `app`

Fetch app metadata by package id.

Input:
- `id` (required): Android package id (e.g., `com.whatsapp`)
- `hl` (optional, default `en`): UI language hint
- `gl` (optional, default `US`): region hint (2-letter country code)
- `output_format` (optional): `raw` | `normalized_v1` | `display_v1`

Examples:
```bash
rzn-tools play-store app --id com.whatsapp
rzn-tools play-store app --id com.whatsapp --output-format normalized_v1
rzn-tools fetch "https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US"
rzn-tools fetch --output-format display_v1 "https://play.google.com/store/apps/details?id=com.whatsapp&hl=en&gl=US"
```

## Best-effort scraping policy

- Data is extracted from the **public** Play Store listing HTML and may change without notice.
- Some fields are locale-dependent (they can vary by `hl`/`gl` and may be missing in some regions).
- When the page is blocked/rate-limited (e.g., HTTP 429), the connector returns a `blocked`-class error.
- Missing fields are returned as `null`/omitted rather than failing the entire request.
