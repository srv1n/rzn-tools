# LinkedIn (`linkedin`)

Official LinkedIn connector for **OAuth/OIDC token import only**. This connector does not use browser cookies, DOM automation, or unofficial scraping.

## Scope

MVP tools:

- `signin`
- `get_auth_status`
- `get_me`
- `create_share_update`
- `create_company_update`
- `api_request`
- `refresh_access_token`

## Auth Model

rzn-tools is a **token consumer**, not the OAuth initiator.

Supply these values from your own backend or OAuth/OIDC broker:

- `access_token`
- `expires_at` (optional)
- `refresh_token` (optional)
- `refresh_token_expires_at` (optional)
- `id_token` (optional)
- `scopes` (optional, space- or comma-delimited)
- `auth_mode` = `member_oauth` or `app_oauth` (optional)
- `organization_urn` (optional)
- `member_urn` (optional)
- `client_id` (optional but recommended when using `id_token` validation or refresh)
- `client_secret` (optional; required for refresh)
- `linkedin_api_version` (optional; defaults to `202603`)

Recommended setup:

```bash
rzn-tools setup linkedin
rzn-tools linkedin auth-status
```

Direct config examples:

```bash
rzn-tools config set linkedin --key access_token --value "<ACCESS_TOKEN>"
rzn-tools config set linkedin --key scopes --value "openid profile email w_member_social"
rzn-tools config set linkedin --key id_token --value "<ID_TOKEN>"
rzn-tools config set linkedin --key organization_urn --value "urn:li:organization:123456"
```

## CLI

```bash
rzn-tools linkedin auth-status
rzn-tools linkedin me
rzn-tools linkedin share --text "Hello LinkedIn"
rzn-tools linkedin share --text "Read this" --url "https://example.com/post"
rzn-tools linkedin company-share --organization urn:li:organization:123456 --text "Company update"
rzn-tools linkedin api-request --method GET --path /v2/userinfo
rzn-tools linkedin refresh-token
```

## Notes

- `get_me` prefers the official `userinfo` endpoint when an access token is available.
- If `userinfo` is unavailable but `id_token` is configured, rzn-tools falls back to ID-token claims and derives `person_urn` as `urn:li:person:<sub>`.
- `create_share_update` supports:
  - text-only posts
  - article/url posts
  - posts referencing an existing LinkedIn media URN via `--image`
- `create_company_update` uses the same official Posts API body shape but requires organization posting permission.
- For richer or newer LinkedIn surfaces, use `api_request` directly.

## Limitations

- No browser-driven automation
- No direct messaging or connection-request automation
- No scraping-based profile/feed access
- No media upload helper in MVP; `--image` expects an existing LinkedIn media URN
