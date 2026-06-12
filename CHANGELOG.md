# Changelog

All notable changes to rzn-tools will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- CLI: add `rzn-tools skills status|install|update|remove` to manage the bundled Agent Skill across project/global scopes and symlink it into Claude Code, Gemini, generic Agent Skills, and Codex skill directories.
- CLI/Packaging: add `rzn-tools workflows list|sync` so installed users can inspect bundled starter workflows/examples, sync the local bundled copy into a managed asset dir, or pull the latest published workflow bundle from GitHub Releases.
- Core/Packaging: bundle connector SVG icons under `resources/icons/connectors`, add a connector-to-icon manifest for downstream repos, and surface those shipped SVGs through `connectors/list.icon_url` and `tools/list.icons`.
- Integrations: add bundle-shipped system metadata and starter quickstarts for auth-backed `openai_web_search` and `exa_search`.
- Docs/Integrations: finish the `rzn-tools` brand cutover and remove the migration-only rename guidance.
- Reddit/Core/CLI: add paginated raw JSON subreddit listings with media archive fields, `--cursor/--after`, `--output-format`, `--include-nsfw`, and a new `reddit media` resolver for gallery/image/video URLs.
- Core: add normalized ingest v1 types/helpers (output_format, cursor encode/decode) for downstream ETL.
- Core: add `whatsapp` (WuzAPI) and `telegram` (grammers/MTProto) connectors behind feature flags.
- Core/CLI: add `google-search-console` and `bing-webmaster-tools` connectors (Search Console / Webmaster Tools APIs) behind feature flags.
- Core/CLI/MCP: add feature-gated `linkedin` connector for official OAuth/OIDC token import, auth status, member identity via userinfo / id_token, official post creation, and raw authenticated LinkedIn API requests.
- CLI/Core: add `--auth-profile` for per-connector multi-account credential sets (stored as `connector::profile`) and optional per-profile `proxy_url` support for Reddit/Hacker News.
- Apple Messages: add persistent local alias management plus alias-only chat/message/send flows so LLM-facing tool use can avoid exposing raw phone numbers and emails.
- Reddit/Hacker News/YouTube/Discord: opt-in `output_format=normalized_v1` outputs for ingestion pipelines.
- Core: add connector metadata methods and a `connectors/list` MCP endpoint for launcher discovery.
- Tools: add schema examples + `_meta` categories for arXiv, Hacker News, YouTube, Reddit, and Wikipedia.
- MCP: support `connectors/list` auth probing via optional `probe_auth` param with safe defaults.
- arXiv: add normalized_v1 outputs plus cursor pagination for search.
- MCP: add `connectors/ingest_sources` with filters and default args for ingestion scheduling.
- CLI: add `ingest` commands to discover sources, configure ingestion, run schedulers, and persist normalized outputs.
- Core: standardize canonical `get` inputs (item_ref/url), add normalized outputs for Phase A connectors, and add fixture-based conformance tests.
- Play Store: add `play-store` connector for best-effort Google Play app metadata from public HTML (plus resolver + CLI subcommand).
- Core/CLI/MCP: add feature-gated `app-store` connector for public App Store metadata (iTunes Search API) and RSS reviews.
- Core/CLI/MCP: add feature-gated `app-store-connect` connector (App Store Connect API, App Analytics reports, Sales & Finance reports).
- Core/CLI/MCP: add feature-gated `apple-search-ads` connector (Apple Search Ads API v5 keyword recommendations + reporting).
- Core/CLI/MCP: add feature-gated `weather` connector (`wttr.in`) with `get_weather`, `units`, `days`, and `output_format=normalized_v1` support.
- Core/CLI/MCP: add feature-gated `polymarket` connector for public tag/event/market/series discovery, event + market retrieval, comments, order books, price history, public positions, bundled market-context reads, and a dedicated `rzn-tools polymarket ...` CLI wrapper for the richer list/analysis flows.
- Core/CLI/MCP: add feature-gated `kalshi` connector for public series/event/market discovery, event metadata, order books, live/historical candles + trades, bundled market-context reads, and a dedicated `rzn-tools kalshi ...` CLI wrapper for richer market-analysis workflows.
- Core/CLI: add `caldav` connector (feature-gated) for calendar discovery and canonical `list`/`get`/`create`/`update`/`delete` event tools (with `output_format=normalized_v1` on `list/get/create/update`), plus provider-specific setup docs.
- Core/CLI: add feature-gated `smtp` connector powered by `lettre` with outbound `send_mail`, `test_connection`, detailed setup/help flows, and connector unit tests.
- Integrations: add bundle-shipped system metadata and starter quickstart assets for `wikipedia`, `youtube_transcripts`, `pubmed`, `reddit`, and `web_search`.

### Changed
- YouTube: `youtube/get` now accepts playlist IDs/URLs and channel handles/URLs and returns ordered `entries[]`; `fetch` routes YouTube playlist/channel URLs to enumeration instead of generic web scraping.
- YouTube: `youtube/list` now uses native YouTube page parsing plus Innertube continuation pagination for channel uploads and playlist videos; omitted `limit` means enumerate until continuations are exhausted.
- CLI: `rzn-tools get <connector> <id> --field <name> --output text` prints scalar fields directly, so YouTube transcripts can be piped without `jq`.
- Licensing: relicense the workspace from dual MIT/Apache-2.0 to AGPL-3.0-only.
- Branding/Packaging: finish the hard cut to `rzn_tools_*`, `rzn-tools`, and `rzn-tools-mcp` across binaries, plugin packaging, install scripts, and release automation.
- Install/Release: `make install`, the shell installer, CI, and GitHub Releases now treat workflow/example assets as first-class payloads alongside the CLI binary, and source installs compile example binaries plus validate bundled workflow metadata before installing.
- Release: add a strict `make release` tag-and-push flow, generate release notes from the git delta with an LLM-backed fallback path, and publish Linux, Windows, macOS Intel, macOS Apple Silicon, workflow assets, and checksums from one GitHub Actions release pass.
- Runtime: remove legacy config/env fallbacks and use `rzn-tools` paths and variables only.
- MCP/Normalized Output: rename wire types to `rzn-tools.normalized_*.v1` and `rzn-tools.display_*.v1`.
- MCP/Integrations: expose `youtube_transcripts/*` and `web_search/*` as first-class system-facing tool namespaces, update quickstarts to call those aliases, and clarify Reddit/web-search system metadata for desktop surfacing.
- MCP/HTTP: curate the remote agent-facing catalog down to canonical task tools, hiding compatibility aliases and `auth/*` setup helpers while keeping old names callable for compatibility.
- MCP/HTTP: expose spec-friendly dotted tool names like `youtube.get` in the remote catalog while continuing to accept slash aliases like `youtube/get` on `tools/call` for compatibility.
- Apple Messages: `list_chats`, `get_recent_messages`, and `send_message` now prefer privacy-safe aliases; `get_recent_messages` also accepts `since` / `since_message_id` and returns a sync cursor for incremental reads.
- Reddit: list/search now accept an opaque cursor for pagination when using normalized output.
- Hacker News: search tools accept `limit`/`cursor` alongside existing page controls for normalized pagination.
- Discord: read_messages now supports cursor-based pagination and normalized channel-window output.
- Discord: normalized `read_messages` now returns a normalized page with top-level `next_cursor/has_more`.
- SciHub connector now performs open-access lookup by DOI (Unpaywall/OpenAlex) and does not bypass paywalls.

### Fixed
- Core: reject non-string `output_format` values instead of silently defaulting to raw output.
- Core/MCP: allow scalar auth details (`string`/`number`/`boolean`/`null`) and coerce scalars to strings (with `null` treated as unset) so typed config fields like IMAP `port: 993` deserialize correctly.
- YouTube: parse channel upload grids that YouTube now serves as `lockupViewModel` items, fixing handle/channel URL enumeration.
- CLI/Polymarket: generic `rzn-tools get polymarket <numeric-id>` now probes event, market, and series reads instead of assuming every numeric id is an event, and returns an explicit ambiguity error when an id exists in multiple namespaces.
- CLI/Kalshi: generic `rzn-tools get kalshi <ticker>` now probes event, market, and series reads and returns an explicit ambiguity error when a ticker exists in multiple namespaces.
- X Browser: fix Chrome/X session auth by using current X web bearer tokens, converting GraphQL GET payloads into query params, and validating browser cookie candidates before use.
- YouTube: make `youtube/list` fall back from broken channel feed URLs to the channel `/videos` page and parse relative publish times there so channel-scoped recent uploads still work.

## [0.2.16] - 2025-12-26

### Fixed
- CI: fix a `dead_code` build failure on newer Rust by marking unused Hacker News types as intentionally unused.

## [0.2.15] - 2025-12-26

### Changed
- CLI: enable a default connector set for source builds so common connectors (PubMed/Wikipedia/etc.) work out of the box.
- CLI: improve “connector not found” errors with actionable `--features ...` rebuild hints.
- Core: allow the `web` connector to compile with `web-lite` (no browser cookie extraction), while preserving cookie extraction when enabled.

## [0.2.14] - 2025-12-24

### Changed
- CLI: exposed pagination and cursor parameters for connectors that now paginate internally (e.g., Reddit `comment_limit`/`comment_sort`, Slack cursors, Google `page_token`/`limit`, Microsoft Graph `next_link`, IMAP pagination).

## [0.2.13] - 2025-12-24

### Changed
- Added a shared pagination helper (`collect_paginated*`) and adopted it across multiple connectors to support higher limits, cursor-based pagination, and de-duplication.
- Improved pagination behavior and limits for connectors that fetch large lists (e.g., Reddit search and Slack/Drive listing) to reduce “first page only” surprises.

## [0.2.12] - 2025-12-22

### Changed
- Fixed `rzn_tools_core` examples to compile again after MCP type refactors (removed stale `async_mcp` usage and aligned examples with `rmcp` request/response types).
- Cleaned up example-only warnings so `cargo test --workspace --all-features` stays green.

## [0.2.11] - 2025-12-22

### Added
- YouTube: `youtube/list` to list recent uploads from a channel/playlist (with `published_within_days` / `published_after`) and `youtube/resolve_channel` to reduce ambiguity when selecting an “official” channel.
- Docs: downstream integration + migration guide (`docs/integrations/DOWNSTREAM_UPGRADE.md`) and updated connector docs to match canonical tool surfaces.

### Changed
- Standardized “tools for agents” surfaces across multiple connectors (canonical `search`/`get`/`list` where applicable) while keeping legacy tool names callable for compatibility.
- Fixed YouTube CLI regressions and aligned YouTube/Reddit/arXiv/PubMed resolver routing to canonical tools.
- Reduced Reddit tool ambiguity by consolidating into `reddit/list`, `reddit/search`, `reddit/get` (with explicit `sort`/`time` parameters on search).

## [0.2.10] - 2025-12-21

### Changed
- Hacker News `top` now uses the official Firebase `topstories` ordering for front-page parity.
- Removed the confusing `rzn-tools call` CLI subcommand; use connector subcommands (e.g., `rzn-tools reddit top ...`) and `rzn-tools tools <connector>`.
- Fixed Reddit CLI wrappers to call the correct underlying tools; `reddit top` now supports `--time` (hour/day/week/month/year/all).

## [0.2.9] - 2025-12-21

### Changed
- Bumped toml dependency to 0.9.10 for wider downstream compatibility

## [0.2.8] - 2025-12-20

### Added
- LLM quick sheet for tool selection (`docs/llms.txt`)
- Task → Tool mappings across connector docs for MCP usage
- Documentation sections for additional connectors (bioRxiv/medRxiv, Google Scholar, RSS, LocalFS, Spotlight, Discord)

### Changed
- Tightened MCP tool descriptions for LLM-friendly selection across connectors
- Updated MCP README tool naming guidance and auth notes
- Clarified explicit user-permission requirements for personal-data connectors

### Added
- Interactive setup wizard (`rzn-tools setup`)
- Comprehensive connector documentation
- GitHub Actions release workflow for all platforms
- Homebrew formula and install script
- Cross-platform binary releases (macOS, Linux, Windows)

### Changed
- Improved CLI help messages and examples
- Updated installation documentation

## [0.1.0] - 2024-XX-XX

### Added

#### Core
- Model Context Protocol (MCP) compliant connector architecture
- Unified `Connector` trait for standardized data source integration
- Thread-safe `ProviderRegistry` for connector management
- Schema-driven authentication system
- Structured error handling with `ConnectorError`

#### CLI (`rzn-tools`)
- `list` - List available connectors
- `search` - Search across connectors
- `get` - Fetch specific content by ID
- `tools` - Show connector tools and parameters
- `config` - Manage authentication
- `setup` - Interactive configuration wizard
- `call` - Call connector tools directly
- Multiple output formats: pretty, JSON, YAML, Markdown

#### MCP Server
- Full MCP protocol compliance
- JSON-RPC over stdio transport
- Tool aggregation across all connectors

#### Connectors

**Media & Social**
- YouTube - Video details, transcripts, chapters, search
- Reddit - Posts, comments, subreddits, user profiles
- X (Twitter) - Tweets, profiles, timelines, DMs
- Hacker News - Stories, comments, search

**Academic & Research**
- arXiv - Paper search and PDF retrieval
- PubMed - Medical literature
- Semantic Scholar - Academic papers with citations
- SciHub - Open-access paper lookup by DOI

**AI-Powered Search**
- OpenAI Web Search (Responses API)
- Anthropic/Claude Web Search
- Gemini Search (Google)
- Perplexity Search
- Tavily, Exa, Firecrawl
- X.AI Grok Search

**Productivity**
- Slack - Channels, messages, files, search
- GitHub - Issues, PRs, code search, files
- Atlassian - Jira issues, Confluence pages

**Google Workspace**
- Gmail - Messages and search
- Calendar - Events and scheduling
- Drive - Files and folders
- People/Contacts

**Microsoft 365**
- Outlook, Teams, OneDrive via Microsoft Graph

**Web Scraping**
- Generic web scraper

**Reference**
- Wikipedia - Articles, search, geo-search

### Security
- Secure credential storage in user config directory
- Browser cookie extraction for authenticated services
- No credentials stored in code or logs
- Environment variable support for all secrets

---

[Unreleased]: https://github.com/srv1n/rzn-tools/compare/v0.2.16...HEAD
[0.2.16]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.16
[0.2.15]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.15
[0.2.14]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.14
[0.2.13]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.13
[0.2.12]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.12
[0.2.11]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.11
[0.2.10]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.10
[0.2.9]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.9
[0.2.8]: https://github.com/srv1n/rzn-tools/releases/tag/v0.2.8
[0.1.0]: https://github.com/srv1n/rzn-tools/releases/tag/v0.1.0
