# Apple Search Ads Connector (`apple-search-ads`)

The `apple-search-ads` connector wraps the **Apple Search Ads API v5**:

- Keyword recommendations (demand proxy / suggested keywords)
- Campaign listing and reporting endpoints

It’s designed for ASO / paid acquisition workflows where you want Search Ads metrics alongside
App Store and App Store Connect data.

## Authentication

Apple Search Ads uses **OAuth client credentials** with a JWT-signed `client_secret` (ES256).

You’ll need:

- `org_id` (Search Ads organization id)
- `oauth_client_id` (OAuth client id)
- `team_id` (Apple Developer Team ID, used as JWT `iss`)
- `key_id` (JWT header `kid`)
- `private_key_path` (path to the downloaded `.p8` private key), or `private_key_p8` (the key text)

### Configure via environment variables

- `ASA_ORG_ID`
- `ASA_OAUTH_CLIENT_ID`
- `ASA_TEAM_ID`
- `ASA_KEY_ID`
- `ASA_P8_PATH` (path to `.p8`)

### Configure via rzn-tools config

```bash
rzn-tools config set apple-search-ads --key org_id --value "123456789"
rzn-tools config set apple-search-ads --key oauth_client_id --value "com.example.searchads.client"
rzn-tools config set apple-search-ads --key team_id --value "ABCDE12345"
rzn-tools config set apple-search-ads --key key_id --value "ABC123DEFG"
rzn-tools config set apple-search-ads --key private_key_path --value "/absolute/path/to/AuthKey_ABC123DEFG.p8"

rzn-tools config test apple-search-ads
```

## Tools

### `keyword_recommendations`

Get keyword recommendations for an app.

Input:
- `app_id` (required): numeric App Store app id
- `storefront_countries` (required): storefront country code(s), e.g. `US`

Example:
```bash
rzn-tools apple-search-ads keyword-recommendations --app-id 310633997 --storefront-countries US
```

### Reporting tools

These tools accept **raw Apple Search Ads report request bodies** (JSON):

- `report_keywords`
- `report_search_terms`
- `report_campaign_keywords`
- `report_campaign_search_terms`

Example:
```bash
rzn-tools apple-search-ads report-keywords --body '{"startTime":"2026-03-01","endTime":"2026-03-03","selector":{"orderBy":[{"field":"taps","sortOrder":"DESCENDING"}]}}'
```

## Notes

- Access tokens are cached in-memory per process. Use `rzn-tools config test apple-search-ads` to
  validate credentials end-to-end.
- Requests automatically retry with backoff on HTTP 429 and 5xx responses.
