# X (Twitter) official API connector (`x`)

Use `x` for the official X platform API itself: reads, timelines, posting, likes, reposts, follows,
bookmarks, and authenticated user-context operations.

If you need cookie-based scraping or richer public thread reconstruction, use `x-browser` instead.

## Auth

`x` supports three auth families on the same connector:

| Mode | Use for |
| --- | --- |
| `bearer_token` | public read endpoints |
| `oauth2_*` | preferred user-context auth |
| `oauth1_*` | fallback for legacy/compatibility paths |

## How To Get X Credentials

Official docs:

- [Getting Access](https://docs.x.com/x-api/getting-started/getting-access)
- [Bearer Tokens](https://docs.x.com/fundamentals/authentication/oauth-2-0/bearer-tokens)
- [OAuth 2.0 overview](https://docs.x.com/fundamentals/authentication/oauth-2-0/overview)
- [OAuth 1.0a overview](https://docs.x.com/fundamentals/authentication/oauth-1-0a/overview)
- [Obtaining OAuth 1.0a user access tokens](https://docs.x.com/resources/fundamentals/authentication/oauth-1-0a/obtaining-user-access-tokens)

Start in the developer portal:

1. Sign in to the X developer portal.
2. Create a Project and App, or open an existing App.
3. Open the App's `Keys and tokens` page.
4. Copy the credential family you actually need.

Credential map:

| You need | Portal source | rzn-tools fields |
| --- | --- | --- |
| Public read access | Bearer Token on `Keys and tokens` | `bearer_token` |
| OAuth 2.0 user-context | Client ID / Secret from the App, plus externally obtained user tokens | `client_id`, `client_secret`, `oauth2_*` |
| OAuth 1.0a fallback | API Key / Secret and Access Token / Secret | `oauth1_consumer_key`, `oauth1_consumer_secret`, `oauth1_access_token`, `oauth1_access_token_secret` |

### Bearer Token

Use bearer when you only need public reads.

Portal steps:

1. Open the App's `Keys and tokens` page.
2. Copy the Bearer Token shown there.

If you need to generate one from API key + secret, X documents the client-credentials flow:

```bash
curl -u "$API_KEY:$API_SECRET_KEY" \
  --data 'grant_type=client_credentials' \
  'https://api.x.com/oauth2/token'
```

Store the returned `access_token` as `bearer_token`.

### OAuth 2.0 User Tokens

Use OAuth 2.0 for user-context reads and writes.

rzn-tools does not launch the X consent screen yet. You obtain the token package externally, then
import it.

Practical steps:

1. In the X portal, enable OAuth 2.0 for the App.
2. Configure your redirect URI in the App settings.
3. Request the scopes you actually need.
4. Run the Authorization Code with PKCE flow outside rzn-tools.
5. Import the resulting access token, refresh token, expiry, and client metadata.

Typical scopes:

| Action | Typical scopes |
| --- | --- |
| `whoami`, user reads | `users.read`, `tweet.read` |
| create/delete posts | `tweet.read`, `tweet.write`, `users.read` |
| likes, follows, bookmarks | `tweet.read`, `tweet.write`, `users.read`, `follows.write`, `like.write`, `bookmark.write` |
| refresh support | add `offline.access` |

### OAuth 1.0a

Use this only if you already have OAuth 1.0a credentials or need a fallback path.

Officially supported ways to get it:

1. Generate Access Token + Access Token Secret for your own account on `Keys and tokens`.
2. Run the 3-legged OAuth 1.0a user flow for another user and exchange the verifier for user access tokens.

### Guided setup flow

Start with the smallest credential that fits the job.

1. Public reads only: import a bearer token.
2. User-context reads or writes: import OAuth 2.0 tokens.
3. Legacy import or endpoint-specific fallback: import OAuth 1.0a tokens.

Recommended validation flow:

```bash
rzn-tools setup x
rzn-tools x auth-status

# Only for user-context auth
rzn-tools x whoami
rzn-tools x refresh-oauth2
```

CLI setup examples:

```bash
rzn-tools setup x
rzn-tools config set x --key bearer_token --value "<X_BEARER_TOKEN>"

rzn-tools config set x --key oauth2_access_token --value "<ACCESS_TOKEN>"
rzn-tools config set x --key oauth2_refresh_token --value "<REFRESH_TOKEN>"
rzn-tools config set x --key oauth2_expires_at --value "1767225599"
rzn-tools config set x --key client_id --value "<CLIENT_ID>"

rzn-tools config set x --key oauth1_consumer_key --value "<CONSUMER_KEY>"
rzn-tools config set x --key oauth1_consumer_secret --value "<CONSUMER_SECRET>"
rzn-tools config set x --key oauth1_access_token --value "<ACCESS_TOKEN>"
rzn-tools config set x --key oauth1_access_token_secret --value "<ACCESS_TOKEN_SECRET>"
```

Import examples by auth family:

```bash
# Bearer only
rzn-tools config set x --key bearer_token --value "<X_BEARER_TOKEN>"

# OAuth 2.0 import
rzn-tools config set x --key oauth2_access_token --value "<ACCESS_TOKEN>"
rzn-tools config set x --key oauth2_refresh_token --value "<REFRESH_TOKEN>"
rzn-tools config set x --key oauth2_expires_at --value "1767225599"
rzn-tools config set x --key oauth2_scope --value "tweet.read users.read tweet.write offline.access"
rzn-tools config set x --key client_id --value "<CLIENT_ID>"

# OAuth 1.0a import
rzn-tools config set x --key oauth1_consumer_key --value "<API_KEY>"
rzn-tools config set x --key oauth1_consumer_secret --value "<API_SECRET>"
rzn-tools config set x --key oauth1_access_token --value "<ACCESS_TOKEN>"
rzn-tools config set x --key oauth1_access_token_secret --value "<ACCESS_TOKEN_SECRET>"
```

Supported config fields:

- `bearer_token`
- `oauth2_access_token`
- `oauth2_refresh_token`
- `oauth2_expires_at`
- `oauth2_scope`
- `oauth2_token_type`
- `client_id`
- `client_secret`
- `redirect_uri`
- `oauth1_consumer_key`
- `oauth1_consumer_secret`
- `oauth1_access_token`
- `oauth1_access_token_secret`
- `base_url`

Environment variables still work:

- `X_BEARER_TOKEN`
- `TWITTER_BEARER_TOKEN`
- `X_OAUTH2_ACCESS_TOKEN`
- `X_OAUTH2_REFRESH_TOKEN`
- `X_OAUTH2_EXPIRES_AT`
- `X_OAUTH2_SCOPE`
- `X_CLIENT_ID`
- `X_CLIENT_SECRET`
- `X_REDIRECT_URI`
- `X_OAUTH_CONSUMER_KEY`
- `X_OAUTH_CONSUMER_SECRET`
- `X_OAUTH_ACCESS_TOKEN`
- `X_OAUTH_ACCESS_TOKEN_SECRET`

For MCP/tool-calling users, the same credential families apply. Set them in the server process
environment or connector auth store, then validate with `x/get_auth_status` and `x/whoami`.

## CLI vs MCP

| Context | What to provide | How to validate |
| --- | --- | --- |
| CLI public reads | `bearer_token` or `X_BEARER_TOKEN` | `rzn-tools x auth-status`, `rzn-tools search x "rust"` |
| CLI user-context | `oauth2_*` plus `client_id` when refresh is needed | `rzn-tools x auth-status`, `rzn-tools x whoami` |
| CLI legacy fallback | `oauth1_*` | `rzn-tools x auth-status`, `rzn-tools x whoami` |
| MCP public reads | same bearer fields in env/auth store | `x/get_auth_status` |
| MCP user-context | same OAuth fields in env/auth store | `x/get_auth_status`, `x/whoami` |

Not this:

- `XAI_API_KEY` is for `xai-search`, not `x`

## Auth routing

rzn-tools prefers:

1. bearer for public reads
2. OAuth 2.0 for user-context operations
3. OAuth 1.0a as fallback

Most tools accept optional `auth_mode`:

- `auto`
- `bearer`
- `oauth2`
- `oauth1`

Use `x/get_auth_status` to inspect what is configured.
Use `x/whoami` to validate user-context auth.

MCP clients should treat `auth_mode=auto` as the default and only pin `bearer`, `oauth2`, or
`oauth1` when a tool or endpoint needs it.

## Tool summary

Read/discovery:

- `x/get_auth_status`
- `x/whoami`
- `x/get_user_by_username`
- `x/get_profile`
- `x/get_tweet`
- `x/search_recent_tweets`
- `x/get_thread`
- `x/get_user_tweets`
- `x/get_user_tweets_by_username`
- `x/get_mentions`
- `x/get_home_timeline`

Writes / social actions:

- `x/create_post`
- `x/delete_post`
- `x/like_post`
- `x/unlike_post`
- `x/repost_post`
- `x/unrepost_post`
- `x/follow_user`
- `x/unfollow_user`
- `x/get_bookmarks`
- `x/add_bookmark`
- `x/remove_bookmark`

## Time filtering

Where supported, rzn-tools accepts:

- RFC3339: `2026-02-24T13:45:00Z`
- Date-only: `YYYY-MM-DD` in UTC

Date-only expansion:

- `start_time: "2026-02-24"` -> `2026-02-24T00:00:00Z`
- `end_time: "2026-02-24"` -> `2026-02-24T23:59:59Z`

Relative lookback:

- `since: "12h"` / `7d` / `4w`

## Notes

- Bearer auth is not enough for posting, likes, reposts, follows, bookmarks, or other user actions.
- If a tool says bearer is insufficient, that is expected. Use OAuth 2.0 or OAuth 1.0a.
- X API billing is credit-based pay-per-usage with endpoint-specific costs tracked in the X developer console.
