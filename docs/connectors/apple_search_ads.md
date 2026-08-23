# Apple Ads Connector (`apple-search-ads`)

The `apple-search-ads` connector supports both the legacy **Apple Search Ads Campaign Management
API v5** and the replacement **Apple Ads Platform API v1**. v5 remains available for existing
workflows; new integrations should use the v1 tools before v5 retires on January 26, 2027.

Platform v1 uses `https://api.ads.apple.com/v1` and scopes every request with
`X-AP-Context: adAccountId=<ad_account_id>;`. It covers App Store and Apple Maps campaigns,
including brands, locations, creatives, reports, insights, recommendations, and change history.

The legacy v5 tools wrap:

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
- `ASA_AD_ACCOUNT_ID` (Platform API v1)
- `ASA_OAUTH_CLIENT_ID`
- `ASA_TEAM_ID`
- `ASA_KEY_ID`
- `ASA_P8_PATH` (path to `.p8`)

### Configure via rzn-tools config

```bash
rzn-tools config set apple-search-ads --key org_id --value "123456789"
rzn-tools config set apple-search-ads --key ad_account_id --value "123456789" # Platform v1
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

## Platform API v1 tools

Use `ad_account_id` for these tools. Bodies are the JSON request objects from Apple’s v1
documentation; query endpoints use `POST` and the common filters/sorting/pagination shape.

| Tool | Endpoint |
|------|----------|
| `platform_query_campaigns` | `POST /campaigns/query` |
| `platform_search_term_popularity` | `POST /insights/apps/search-term-popularity/query` |
| `platform_impression_share` | `POST /insights/apps/impression-share/query` |
| `platform_recommendations` | `POST /recommendations/{daily-budgets,target-cpas}/query` |
| `platform_report_apps` / `platform_report_brands` | `POST /reports/{apps,business-brands}/{level}/query` |
| `platform_query_brands` / `platform_query_locations` | `POST /business-brands/query`, `/locations/query` |
| `platform_query_creatives` | `POST /creatives/query` |
| `platform_change_history` | `POST /change-history/query` |
| `platform_request` | Any documented v1 relative path (`GET`, `POST`, `PUT`, `DELETE`) |

Examples:

```bash
rzn-tools apple-search-ads platform-query-campaigns --body '{"pagination":{"offset":0,"pageSize":20}}'
rzn-tools apple-search-ads platform-search-term-popularity --body '{"filters":[]}'
rzn-tools apple-search-ads platform-request --method GET --path /me
```

The raw request tool is intentionally path- and method-validated, so it can cover newly added
v1 endpoints without allowing arbitrary URLs, including campaign-group management and creative
mutations. Binary asset uploads remain outside this JSON-only tool until a local-file contract is
needed.

## Notes

- Access tokens are cached in-memory per process. Use `rzn-tools config test apple-search-ads` to
  validate credentials end-to-end.
- Requests automatically retry with backoff on HTTP 429 and 5xx responses.
