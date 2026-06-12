# Launcher Integration Implementation Summary

## Scope Delivered
- Connector trait metadata methods added and implemented across all connectors:
  `display_name`, `icon`, `categories`, `requires_auth`, `url_patterns`.
- New MCP endpoint: `connectors/list` for launcher discovery.
- New MCP endpoint: `connectors/ingest_sources` for generic ingestion configuration.
- Tool input schema examples + `_meta` category/tags for core connectors:
  arXiv, Hacker News, YouTube, Reddit, Wikipedia.

## `connectors/list` Behavior
- Response shape: `connectors[]` with name, display_name, description, icon, tools_count, tools,
  categories, url_patterns, auth_required, auth_status.
- Auth status mapping:
  - `not_required` when `requires_auth == false`
  - `ready` when `test_auth()` succeeds
  - `invalid` on `ConnectorError::Authentication`
  - `needs_setup` on `ConnectorError::InvalidInput` / `InvalidParams`
  - `unknown` otherwise
- Auth probe skip list (avoids personal-data access during discovery):
  `apple-mail`, `apple-notes`, `apple-messages`, `apple-reminders`, `apple-contacts`,
  `google-gmail`, `google-people`, `imap`, `microsoft-graph`.

## Tool Schema Enhancements
Examples and `_meta` fields are now included for core connectors:
- arXiv: `search`, `get`
- Hacker News: `search`, `search_by_date`, `get_stories`, `get_post`
- YouTube: `get`, `search`, `list`, `resolve_channel`
- Reddit: `list`, `search`, `get`
- Wikipedia: `search`, `geosearch`, `get_article`

`_meta` includes:
- `category` (e.g., `search`, `read`, `list`, `resolve`)
- `tags` (connector-specific)
- `auth_required`

## Files Touched (Highlights)
- `rzn_tools_core/src/lib.rs`: connector metadata defaults, URLPatternSpec types
- `rzn_tools_core/src/mcp_server.rs`: `connectors/list` structs + handler + JSON-RPC routing
- `rzn_tools_core/src/mcp_server.rs`: `connectors/ingest_sources` aggregation + JSON-RPC routing
- `rzn_tools_core/src/connectors/*/mod.rs`: metadata methods for all connectors
- `rzn_tools_core/src/connectors/{arxiv,hackernews,youtube,reddit,wikipedia}/mod.rs`: examples + `_meta`
- `rzn_tools_core/tests/launcher_integration.rs`: connectors/list test + arXiv examples test
- `CHANGELOG.md`: launcher integration additions

## Tests Added
- `rzn_tools_core/tests/launcher_integration.rs`
  - Validates `connectors/list` shape with a dummy connector
  - Validates `connectors/ingest_sources` defaults + filters with a dummy connector
  - Validates arXiv tool schema examples (feature-gated on `arxiv`)

## Notes for Reviewers
- All connector icons use ASCII identifiers (no emoji).
- URL patterns are present for key connectors (YouTube, Reddit, HN, arXiv, Wikipedia, etc.).
- Core tool examples are intentionally scoped to the 5 primary connectors for launcher UX.
- `connectors/ingest_sources` semantics + params are documented in:
  `docs/integrations/INGEST_SOURCES_ENDPOINT_SPEC.md`.
