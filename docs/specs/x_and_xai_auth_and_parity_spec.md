# X connector parity + xAI search parity

Build out the existing `x` and `xai-search` connectors instead of inventing new connector families.

Core decisions:

- Keep `x` for the official X platform API
- Keep `xai-search` for xAI `web_search` and `x_search` only
- Do not merge them
- Do not add generic Grok inference tools
- Expand `x` aggressively toward official API parity
- Expand `xai-search` only to search-tool parity

Reference:

- `docs/connectors/x_auth_rfc.md`
- `docs/connectors/xmcp-gap-analysis.md`

## Story 1: Extend `x` auth schema for bearer + OAuth 2.0 + OAuth 1.0a

### Type
feature

### Priority
1

### Description
Extend the existing `x` connector config/auth schema so it can store bearer credentials, OAuth 2.0 PKCE user tokens, and OAuth 1.0a user tokens on the same connector without introducing a new abstraction layer.

### Design
- Update `rzn_tools_core/src/connectors/x/mod.rs`
- Add explicit config fields for:
  - `bearer_token`
  - `oauth2_access_token`
  - `oauth2_refresh_token`
  - `oauth2_expires_at`
  - `oauth2_scope`
  - `oauth2_token_type`
  - `client_id`
  - optional `client_secret`
  - optional `redirect_uri`
  - `oauth1_consumer_key`
  - `oauth1_consumer_secret`
  - `oauth1_access_token`
  - `oauth1_access_token_secret`
- Keep backward compatibility with existing `bearer_token`

### Acceptance Criteria
- `x` config schema exposes all three auth families
- Existing bearer-token-only usage still works
- `get_auth_details` and `set_auth_details` round-trip the new fields cleanly
- Docs mention supported auth inputs without ambiguity

### Labels
auth

## Story 2: Implement auth mode selection + auth diagnostics in `x`

### Type
feature

### Priority
1

### Description
Add internal auth selection logic so `x` can choose bearer, OAuth 2.0, or OAuth 1.0a based on endpoint requirements, and expose diagnostics so users can tell what is configured and what is missing.

### Design
- Add endpoint auth metadata in `rzn_tools_core/src/connectors/x/mod.rs` or a nearby helper module
- Prefer:
  - bearer for public reads
  - OAuth 2.0 for user-context endpoints
  - OAuth 1.0a as fallback
- Add one or both:
  - `auth_status`
  - `whoami`

### Acceptance Criteria
- Internal request building can choose the correct auth mode per operation
- Missing-required-auth errors are explicit and actionable
- `auth_status` reports configured auth families and token freshness at a high level
- `whoami` works for configured user-context auth

### Labels
auth

## Story 3: Add internal spec-backed operation registry for `x`

### Type
feature

### Priority
1

### Description
Replace the tiny hand-curated-only implementation with an internal operation registry that can map official X operations to request definitions and auth requirements, while keeping the connector name and overall shape as `x`.

### Design
- Do not generate a new public connector
- Add an internal registry/module for operation metadata
- Support:
  - operation id
  - method
  - path
  - query/body parameter schema
  - auth requirements
- Start from the high-value operations first, not the entire world

### Acceptance Criteria
- `x` has an internal operation registry rather than only ad hoc handlers
- Operation definitions can express method/path/auth requirements
- Curated tools can delegate through the registry
- Registry design does not block adding more operations later

### Labels
platform

## Story 4: Expand `x` read coverage to high-value official endpoints

### Type
feature

### Priority
1

### Description
Expand the `x` connector’s curated public read surface beyond recent search and single-post lookups.

### Design
Implement curated tools for:
- `get_post`
- `get_posts`
- `search_recent_posts`
- `search_all_posts`
- `get_user`
- `get_users`
- `get_user_posts`
- `get_mentions`
- `get_home_timeline`
- `get_usage`

### Acceptance Criteria
- Curated read tools exist for the listed operations or documented equivalent names
- Public lookup/search tools work with bearer auth where supported
- User-context timeline/mentions tools require user auth and fail cleanly otherwise
- Tool docs/examples reflect the expanded surface

### Labels
read

## Story 5: Expand `x` write coverage to high-value official endpoints

### Type
feature

### Priority
1

### Description
Add the main platform actions people actually want instead of pretending a read-only connector is enough.

### Design
Implement curated tools for:
- `create_post`
- `delete_post`
- `like_post`
- `unlike_post`
- `repost_post`
- `unrepost_post`
- `follow_user`
- `unfollow_user`
- `get_bookmarks`
- `add_bookmark`
- `remove_bookmark`

### Acceptance Criteria
- Curated write tools exist for the listed operations or documented equivalent names
- These tools reject bearer-only auth with a clear error
- OAuth 2.0 is the preferred auth path for these tools
- OAuth 1.0a can be used where implemented as fallback

### Labels
write

## Story 6: Add list/DM/media support in `x`

### Type
feature

### Priority
2

### Description
Close the next biggest parity gaps in `x`: lists, direct messages, and media-related operations.

### Design
Implement curated tools for:
- list operations:
  - `list_lists`
  - `create_list`
  - `update_list`
  - `delete_list`
- DM operations:
  - `list_dm_conversations`
  - `get_dm_messages`
  - `send_dm`
- media helpers needed by posting/DM flows

### Acceptance Criteria
- `x` supports at least one practical path through lists, DMs, and media
- User-context auth is required and enforced
- Media upload flow is documented if multi-step
- Tool responses are normalized enough for MCP use

### Labels
dm,media,lists

## Story 7: Add raw official operation fallback inside `x`

### Type
task

### Priority
2

### Description
Provide a controlled raw operation path so `x` can hit uncovered official operations without waiting for a bespoke curated wrapper every time.

### Design
Suggested tool:
- `raw_operation`

Inputs:
- `operation_id`
- `params`
- optional `auth_mode`

Keep it constrained by the internal operation registry rather than arbitrary URL fetches.

### Acceptance Criteria
- `x/raw_operation` or equivalent exists
- It only allows known registered operations
- It uses the same auth selection logic as curated tools
- It returns raw-ish structured data without losing status/error detail

### Labels
raw

## Story 8: Add refresh/import flows for OAuth 2.0 in `x`

### Type
feature

### Priority
2

### Description
Support practical OAuth 2.0 user token management in `x`, starting with token import/refresh rather than a fully polished browser setup flow.

### Design
- Add import semantics via config/auth details
- Add refresh path when `oauth2_refresh_token` and `client_id` are present
- Persist refreshed access token details back into connector auth state if the surrounding system allows it

### Acceptance Criteria
- Imported OAuth 2.0 user tokens can be used without manual header hacks
- Refresh occurs when token expiry is reached and refresh data is available
- Refresh failures produce a clear reauth-required error
- Docs explain what token material needs to be provided

### Labels
oauth2

## Story 9: Add OAuth 1.0a fallback support in `x`

### Type
feature

### Priority
2

### Description
Implement OAuth 1.0a request signing for legacy or compatibility cases where OAuth 2.0 is unavailable, undesired, or not yet sufficient.

### Design
- Add signing helper for OAuth 1.0a requests
- Use only when endpoint selection logic chooses it or caller explicitly requests it
- Do not let OAuth 1.0a dominate the design

### Acceptance Criteria
- OAuth 1.0a signed requests can be issued for supported operations
- OAuth 1.0a credentials are validated before request dispatch
- Errors distinguish signing/config issues from endpoint failures
- OAuth 2.0 remains the preferred user-context path where both work

### Labels
oauth1

## Story 10: Bring `xai-search` to search-tool parity

### Type
feature

### Priority
1

### Description
Keep `xai-search` narrow, but make it complete for the search-tool use case.

### Design
Add support for:
- `allowed_x_handles`
- `excluded_x_handles`
- `enable_image_understanding`
- `enable_video_understanding`
- explicit `from_date`
- explicit `to_date`
- existing web domain allow/block filters
- citations passthrough
- usage passthrough

Do not add generic chat/completions features.

### Acceptance Criteria
- `xai-search/search` supports the listed xAI `x_search` parameters where the official API supports them
- Existing `sources=["web","x"]` behavior still works
- Search results continue to surface citations and usage
- Connector docs clearly state that this is search-only, not generic inference

### Labels
search

## Story 11: Update docs, pricing notes, and examples

### Type
task

### Priority
2

### Description
Bring docs back in sync with reality after the connector expansion.

### Design
Update:
- `docs/connectors/x.md`
- `docs/connectors/xai_search` docs if added or existing related docs
- pricing notes where relevant
- CLI examples
- setup/auth examples

### Acceptance Criteria
- `x` docs explain bearer vs OAuth 2.0 vs OAuth 1.0a clearly
- `xai-search` docs explain `XAI_API_KEY` and search-only scope clearly
- Pricing notes reflect:
  - X API credit-based pay-per-usage
  - xAI tool-call billing plus tokens
- Examples are consistent with actual tool names and fields

### Labels
docs

## Story 12: Add tests for auth selection and endpoint behavior

### Type
task

### Priority
1

### Description
Lock the auth and routing behavior down with tests so this does not regress into credential spaghetti.

### Design
Add unit and fixture-style tests for:
- auth mode selection
- missing credential errors
- bearer vs user-context tool gating
- OAuth 1.0a signing helper behavior
- xAI search body construction

### Acceptance Criteria
- Tests cover auth mode selection for representative read and write operations
- Tests verify `xai-search` request bodies for new parity params
- No tests require live network access
- Failures point to connector behavior, not vague integration soup

### Labels
tests
