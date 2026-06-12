# App Store Connector (`app-store`)

The `app-store` connector fetches **public** App Store app metadata via the **iTunes Search API**
and recent reviews via the **App Store RSS feed**.

This is useful for ASO workflows (metadata, positioning, review monitoring) without requiring any
developer credentials.

## Tools

### `search`

Search apps by keyword.

Input:
- `query` (required): search terms
- `country` (optional, default `US`): storefront country code (ISO 2-letter)
- `limit` (optional, default `25`, max `200`): number of results

Examples:
```bash
rzn-tools app-store search --query "habit tracker" --limit 10
rzn-tools search app-store "habit tracker" --limit 10
```

### `lookup`

Lookup app details by App Store `track_id` (aka “adam id”).

Input:
- `track_id` (required): numeric App Store track id
- `country` (optional, default `US`): storefront country code (ISO 2-letter)

Examples:
```bash
rzn-tools app-store lookup --track-id 310633997
rzn-tools get app-store 310633997
rzn-tools fetch https://apps.apple.com/us/app/id310633997
```

### `reviews`

Fetch recent customer reviews via the public RSS feed (JSON).

Input:
- `track_id` (required): numeric App Store track id

Example:
```bash
rzn-tools app-store reviews --track-id 310633997
```

### `test_auth`

Smoke test API connectivity.

Example:
```bash
rzn-tools app-store test-auth
```

## Notes

- Storefronts can vary in available fields; use `country` when you need a specific storefront.
- Reviews are fetched from the RSS feed, which is public and may not include *all* reviews.
