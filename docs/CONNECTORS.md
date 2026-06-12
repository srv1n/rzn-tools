# RZN Integrations Connector Reference

> Capability-first connector docs for RZN Integrations (`rzn-tools`)

**MCP tool names are prefixed with connector name**, e.g. `youtube/search`.

---

## Quick Navigation

| Category | Connectors |
|----------|------------|
| [Media & Social](#media--social) | YouTube, Reddit, LinkedIn, X (Twitter), Hacker News, Slack, Discord, WhatsApp, Telegram |
| [Academic & Research](#academic--research) | arXiv, PubMed, Semantic Scholar, SciHub |
| [Web Search](#web-search) | Serper, SerpAPI, Tavily, + more |
| [AI-Powered Search](#ai-powered-search) | OpenAI, Anthropic, Gemini, Perplexity |
| [Productivity](#productivity) | CalDAV, Slack, GitHub, Atlassian, IMAP, SMTP |
| [Google Workspace](#google-workspace) | Gmail, Calendar, Drive, Contacts |
| [SEO & Search Console](#seo--search-console) | Google Search Console, Bing Webmaster Tools |
| [Microsoft 365](#microsoft-365) | Outlook, Teams, OneDrive |
| [Markets & Forecasting](#markets--forecasting) | Polymarket, Kalshi |
| [Web Scraping](#web-scraping) | Generic web |
| [Reference](#reference) | Wikipedia |
| [App Stores](#app-stores) | App Store, App Store Connect, Apple Search Ads, Play Store |

---

## Media & Social

### LinkedIn (`linkedin`)
> Official LinkedIn OAuth/OIDC connector for auth status, member identity, post creation, and raw authenticated API requests. This connector does not use browser cookies or browser automation.

| Tool | Description |
|------|-------------|
| `signin` | Import externally obtained OAuth/OIDC material into the connector session |
| `get_auth_status` | Show scopes, expiry, refresh availability, and cached member/org identifiers |
| `get_me` | Resolve the authenticated member via `userinfo` or the configured `id_token` |
| `create_share_update` | Create a member-authored LinkedIn post |
| `create_company_update` | Create an organization-authored LinkedIn post |
| `api_request` | Make a raw authenticated LinkedIn API request |
| `refresh_access_token` | Refresh the access token when refresh-token support is configured |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Check whether imported tokens are usable | `linkedin/get_auth_status` |
| Resolve the authenticated member + person URN | `linkedin/get_me` |
| Create a member post | `linkedin/create_share_update` |
| Create an organization/company post | `linkedin/create_company_update` |
| Call a supported LinkedIn endpoint directly | `linkedin/api_request` |
| Refresh an expiring token | `linkedin/refresh_access_token` |

**Auth:** External OAuth/OIDC token import only. rzn-tools acts as a token consumer and does not launch the LinkedIn browser consent flow itself.

**Notes:**
- `access_token` is the primary credential.
- `id_token` is optional and is used for member identity / `person_urn` derivation.
- `refresh_token` support depends on your LinkedIn partner entitlements; when refresh fails or is unavailable, the connector surfaces `reauth_required`.
- `create_share_update` requires `w_member_social`.
- `create_company_update` requires `w_organization_social` plus an organization URN.
- `api_request` automatically adds Rest.li headers for `/rest/...` endpoints and defaults `Linkedin-Version` to `202603` unless overridden.

### YouTube
> Video details, transcripts, chapters, and search

| Tool | Description |
|------|-------------|
| `get` | Fetch video metadata + transcript (chapters when available) |
| `search` | Search videos/playlists/channels (use `search_type`) |
| `list` | List channel uploads or playlist videos with native YouTube pagination |
| `resolve_channel` | Resolve a channel name/handle/url to a stable UC... channel ID |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Video details + transcript | `youtube/get` |
| Search videos/playlists/channels | `youtube/search` |
| List channel uploads / playlist videos | `youtube/list` |
| Resolve an "official" channel | `youtube/resolve_channel` |

**Features:**
- Automatic transcript extraction with chapter grouping
- Search filters: upload date, sort order, content type
- Native continuation pagination for channel uploads and playlist videos
- No authentication required

Note: `youtube/search` supports `search_type="video"|"playlist"|"channel"` for discovery, but
`youtube/get` operates on a **single video** (video ID or URL). Use `youtube/list` to bridge from a
channel/playlist to concrete video IDs/URLs. Omit `limit` to enumerate until YouTube stops returning
continuation pages; pass `limit` for last-N channel uploads or first-N playlist items.

**Example:**
```bash
rzn-tools get youtube "dQw4w9WgXcQ"
rzn-tools search youtube "rust programming" --limit 10
```

---

### Reddit
> Posts, comments, subreddits, and user profiles

| Tool | Description |
|------|-------------|
| `list` | Browse a subreddit feed (hot/new/top) |
| `search` | Keyword search (optionally scoped to a subreddit) |
| `get` | Post + comments by `post_url` |
| `media` | Resolve ordered media URLs for a post |
| `user` | User profile metadata (karma + account creation time) |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Subreddit feed (hot/new/top) | `reddit/list` |
| Keyword search | `reddit/search` |
| Post + comments | `reddit/get` |
| Post media URLs | `reddit/media` |
| User profile metadata | `reddit/user` |

**Output formats:**
- Many tools accept `output_format=raw|normalized_v1|display_v1`.

**Features:**
- Works anonymously or with authentication
- `list` supports cursor pagination, `limit` up to 5000, `include_nsfw`, and media-heavy raw fields (`id`, `is_gallery`, `gallery_data`, `media_metadata`, `preview`, crossposts, etc.)
- `media` resolves galleries, direct images, hosted videos, crosspost media, and external links into ordered URL entries tagged as `image`, `animated`, `video`, or `external`
- Comment threading with configurable depth
- Search by author, subreddit, flair, domain

**Limitations & privacy notes:**
- `reddit/user` only returns public metadata from the `about.json` endpoint (no private content).
- Reddit may rate-limit or IP-block anonymous JSON requests; configure `proxy_url` or `api_base_url=https://old.reddit.com` when needed.
- Missing/deleted/suspended users return a non-panicking error.

**Authentication:** Optional (Client ID + Secret for higher rate limits)

---

## App Stores

## Markets & Forecasting

### Polymarket (`polymarket`)
> Public read-only access to Polymarket discovery, market analysis, and high-context prediction-market workflows.

| Group | Tools | Description |
|------|-------|-------------|
| Discovery | `search`, `list_tags`, `list_events`, `list_markets`, `list_series` | Find relevant markets by topic, discover usable tag slugs, or browse by series, tag, and status |
| Entity reads | `get`, `get_market`, `get_series` | Fetch one event, market, or series |
| Discussion | `list_comments` | Pull comments for an event, market, or series |
| Analysis | `order_book`, `price_history`, `market_positions` | Inspect order flow, prices, and public holder data |
| High-context | `get_market_context` | Fetch a market with linked event, order books, price history, and optional positions |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Discover active events | `polymarket/search` |
| Discover valid tag slugs | `polymarket/list_tags` |
| Browse events by tag or series | `polymarket/list_events` |
| Expand one event into all of its contracts | `polymarket/list_markets` |
| Open an event page or event slug | `polymarket/get` |
| Inspect a single market | `polymarket/get_market` |
| Pull comments for a market or event | `polymarket/list_comments` |
| Inspect market microstructure | `polymarket/order_book`, `polymarket/price_history`, `polymarket/market_positions` |
| Get one analysis-ready bundle | `polymarket/get_market_context` |

**Notes:**
- No authentication required.
- `polymarket/search` paginates internally because the public search endpoint currently returns small fixed pages.
- `list_tags`, `list_events`, `list_markets`, `list_series`, and `list_comments` expose cursors for pagination.
- `order_book` and `price_history` automatically resolve CLOB token ids from market metadata.
- `get_market_context` is the preferred analysis tool when an agent needs one market plus its linked event, price trajectory, and book state in a single response.
- The CLI also exposes these richer flows directly via `rzn-tools polymarket ...` for list + analysis workflows.
- `rzn-tools fetch https://polymarket.com/event/...` routes to `polymarket/get` via the smart resolver when the feature is enabled.
- Use `output_format=normalized_v1` or `display_v1` when you want ingestion-friendly or UI-friendly results.

### Kalshi (`kalshi`)
> Public read-only access to Kalshi series, events, markets, order books, candlesticks, trades, and bundled market context.

| Group | Tools | Description |
|------|-------|-------------|
| Discovery | `search`, `list_series`, `list_events`, `list_markets` | Search across live public prediction data, browse series catalogs, list events, or browse live/historical markets |
| Entity reads | `get_series`, `get`, `get_market` | Fetch one series, one event, or one market |
| Event context | `get_event_metadata`, `event_candlesticks` | Pull settlement sources, imagery, and event-level market candles |
| Market microstructure | `order_book`, `market_candlesticks`, `list_trades` | Inspect book depth, price history, and recent fills |
| High-context | `get_market_context` | Fetch one market plus parent event, parent series, routing metadata, trades, candles, and optional event metadata |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Search public Kalshi contracts by topic | `kalshi/search` |
| Browse the series catalog | `kalshi/list_series` |
| Open a Kalshi event page URL | `kalshi/get` |
| Pull settlement sources for an event | `kalshi/get_event_metadata` |
| Browse all markets under an event or series | `kalshi/list_markets` |
| Inspect one exact market ticker | `kalshi/get_market` |
| Check live book depth | `kalshi/order_book` |
| Review market or event-level candles | `kalshi/market_candlesticks`, `kalshi/event_candlesticks` |
| Review recent trades | `kalshi/list_trades` |
| Hand an agent one analysis-ready bundle | `kalshi/get_market_context` |

**Notes:**
- No authentication is required.
- `get_market`, `market_candlesticks`, and `list_trades` automatically fall back to Kalshi's historical storage when a market is older than the public historical cutoff.
- `list_series` uses client-side cursor pagination because the public series catalog is exposed as one list.
- `list_events`, `list_markets`, and `list_trades` expose provider cursors for pagination.
- `get_market_context` is the preferred analysis tool when an agent needs parent event + series context, recent trades, candles, and book state in a single response.
- The CLI also exposes these richer flows directly via `rzn-tools kalshi ...`.
- `rzn-tools fetch https://kalshi.com/markets/.../<event-ticker>` routes to `kalshi/get` via the smart resolver when the feature is enabled.
- Use `output_format=normalized_v1` or `display_v1` when integrating with ingestion/UI pipelines.

---

## App Stores

### App Store (`app-store`)
> Public App Store metadata via the iTunes Search API (plus reviews via the RSS feed).

| Tool | Description |
|------|-------------|
| `search` | Search apps by keyword |
| `lookup` | Lookup app details by `track_id` (adam id) |
| `reviews` | Fetch recent customer reviews via the public RSS feed |
| `test_auth` | Smoke test iTunes Search API connectivity |

**Authentication:** Not required

**Notes:**
- `apps.apple.com/.../id123` URLs are routed to `app-store/lookup` via the smart resolver.

### App Store Connect (`app-store-connect`)
> App Store Connect API for app lists, App Analytics report segments, and Sales/Finance reports.

| Tool | Description |
|------|-------------|
| `list_apps` | List apps in your App Store Connect account |
| `get_app` | Fetch a single app by App Store Connect app id |
| `create_analytics_report_request` | Create a report request for an app (`ONE_TIME_SNAPSHOT` or `ONGOING`) |
| `list_analytics_reports` | List reports for a report request id |
| `list_analytics_report_instances` | List instances for a report id (filter by date/granularity) |
| `list_analytics_report_segments` | List downloadable segments for an instance id |
| `download_analytics_report_segment` | Download a segment URL and return a bounded preview (often gzip TSV) |
| `download_sales_report` | Download a Sales report (gzip TSV) |
| `download_finance_report` | Download a Finance report (gzip TSV) |
| `test_auth` | Validate JWT signing + API access |

**Authentication:** Required (App Store Connect API key JWT)

You can configure auth via either:
- Environment variables:
  - `APP_STORE_CONNECT_KEY_ID`
  - `APP_STORE_CONNECT_ISSUER_ID`
  - `APP_STORE_CONNECT_P8_PATH` (path to `.p8` key)
  - Optional: `APP_STORE_CONNECT_VENDOR_NUMBER`
- Or saved config:
  - `rzn-tools config set app-store-connect --key key_id --value ...`
  - `rzn-tools config set app-store-connect --key issuer_id --value ...`
  - `rzn-tools config set app-store-connect --key private_key_path --value /path/to/AuthKey_XXXXXX.p8`

**Notes:**
- Report downloads are size-guarded; increase `max_kb`/`max_uncompressed_kb` if you expect large reports.

### Apple Search Ads (`apple-search-ads`)
> Apple Search Ads API v5 for keyword recommendations and reporting.

| Tool | Description |
|------|-------------|
| `list_campaigns` | List campaigns |
| `keyword_recommendations` | Get keyword recommendations for an app |
| `report_keywords` | Keyword reporting (POST `/reports/keywords`) |
| `report_search_terms` | Search terms reporting (POST `/reports/searchterms`) |
| `report_campaign_keywords` | Campaign keyword reporting |
| `report_campaign_search_terms` | Campaign search terms reporting |
| `create_campaign` | Create a campaign |
| `test_auth` | Validate OAuth token + API access |

**Authentication:** Required (OAuth client credentials + ES256 `.p8` key)

You can configure auth via either:
- Environment variables:
  - `ASA_ORG_ID`
  - `ASA_OAUTH_CLIENT_ID`
  - `ASA_TEAM_ID`
  - `ASA_KEY_ID`
  - `ASA_P8_PATH` (path to `.p8` key)
- Or saved config:
  - `rzn-tools config set apple-search-ads --key org_id --value ...`
  - `rzn-tools config set apple-search-ads --key oauth_client_id --value ...`
  - `rzn-tools config set apple-search-ads --key team_id --value ...`
  - `rzn-tools config set apple-search-ads --key key_id --value ...`
  - `rzn-tools config set apple-search-ads --key private_key_path --value /path/to/key.p8`

### Play Store (`play-store`)
> Best-effort Google Play app metadata via public HTML parsing (no auth required).

| Tool | Description |
|------|-------------|
| `app` | App metadata by package id (`id=com.whatsapp`) |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| App metadata | `play-store/app` |

**Notes:**
- Output is best-effort: fields may be missing or change with locale (`hl`/`gl`) and markup changes.
- Use `rzn-tools fetch https://play.google.com/store/apps/details?id=...` to rely on the smart resolver.
- Use `output_format=normalized_v1` for ingestion pipelines or `output_format=display_v1` for UI-friendly output.

---

### X (Twitter)
> Official X API v2 (bearer, OAuth 2.0, OAuth 1.0a)

For detailed, LLM-friendly calling patterns (MCP + CLI), including **RFC3339 date/time filters**
and **pagination token usage**, see `docs/connectors/x.md`.

| Tool | Description |
|------|-------------|
| `get_profile` | Alias for `get_user_by_username` (profile by username) |
| `get_user_by_username` | Get user metadata by username |
| `search_recent_tweets` | Search recent tweets (token pagination) |
| `get_tweet` | Get a tweet by ID |
| `get_thread` | Recent conversation snapshot for a tweet (token pagination) |
| `get_user_tweets` | Get a user's tweets (token pagination) |
| `get_user_tweets_by_username` | Convenience: username → user_id → tweets |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Auth diagnostics | `x/get_auth_status`, `x/whoami` |
| Recent search | `x/search_recent_tweets` |
| Full-archive search | `x/search_all_tweets` |
| Tweet details | `x/get_tweet` |
| User/profile lookup | `x/get_profile` |
| Thread snapshot | `x/get_thread` |
| User tweets | `x/get_user_tweets` |
| Mentions / home timeline | `x/get_mentions`, `x/get_home_timeline` |
| Post / social actions | `x/create_post`, `x/like_post`, `x/repost_post`, `x/follow_user`, `x/add_bookmark` |
| Lists / DMs / media | `x/create_list`, `x/create_dm_conversation`, `x/initialize_media_upload` |
| Spec fallback | `x/raw_operation` |

**Authentication:** bearer for public reads; OAuth 2.0 PKCE for user-context reads/writes; OAuth 1.0a as fallback when importing legacy tokens or when an endpoint requires it.

```bash
rzn-tools setup x
rzn-tools x auth-status
rzn-tools x whoami
```

For token import and field-by-field setup, see `docs/connectors/x.md`.

---

### X (Browser Cookies)
> Scraper-based X access via browser cookies (threads/conversation context)

For detailed, LLM-friendly calling patterns (MCP + CLI), including **date/time filters** and
**pagination cursor usage**, see `docs/connectors/x_browser.md`.

| Tool | Description |
|------|-------------|
| `get_profile` | Get user profile information |
| `search_tweets` | Search tweets by keyword |
| `get_tweet` | Get tweet details by ID (supports URL auto-resolve) |
| `get_thread` | Get a tweet thread/conversation by tweet_id |
| `get_home_timeline` | Get authenticated user's feed |
| `fetch_tweets_and_replies` | Get all tweets from a user |
| `get_user_tweets` | Get a user's tweets (no replies) |
| `search_profiles` | Search for user profiles |
| `get_followers` | Get user's followers list |
| `get_direct_message_conversations` | Access DM threads |
| `send_direct_message` | Send DMs (authenticated) |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| User profile | `x-browser/get_profile` |
| Keyword search | `x-browser/search_tweets` |
| Tweet details | `x-browser/get_tweet` |
| Thread/conversation | `x-browser/get_thread` |

**Authentication:** Required (browser cookies or credentials)

```bash
rzn-tools setup x-browser
rzn-tools config set x-browser --browser chrome
```

---

### Hacker News
> Tech news, discussions, and job postings

| Tool | Description |
|------|-------------|
| `search` | Canonical relevance-ranked thread search |
| `search_recent` | Canonical recent chronological thread search |
| `list_threads` | Canonical feed listing (top/new/best/ask/show/job) |
| `get_thread` | Canonical thread fetch with compact plain-text output |
| `search_stories` | Legacy alias for `search` |
| `search_by_date` | Legacy alias for `search_recent` |
| `get_stories` | Legacy alias for `list_threads` |
| `get` / `get_post` | Legacy aliases for `get_thread` |

**Features:**
- Powered by Algolia search API
- Compact LLM-friendly thread output with bounded comments
- Flattened or nested comment trees
- No authentication required

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Top/new/best/ask/show/job stories | `hackernews/list_threads` |
| Keyword search | `hackernews/search` |
| Recent chronological search | `hackernews/search_recent` |
| Story with comments | `hackernews/get_thread` |

---

## Academic & Research

### arXiv
> Preprints in physics, mathematics, computer science, and more

| Tool | Description |
|------|-------------|
| `search` | Search arXiv by query |
| `get` | Paper metadata by arXiv ID |

**Features:**
- Field-specific search: `ti:` (title), `au:` (author), `abs:` (abstract)
- Sort by relevance, submission date, or update date
- No authentication required

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Search papers | `arxiv/search` |
| Paper details | `arxiv/get` |

**Example:**
```bash
rzn-tools search arxiv "au:hinton AND ti:neural"
```

---

### PubMed
> Biomedical and life sciences literature

| Tool | Description |
|------|-------------|
| `search` | Search PubMed |
| `get` | Abstract + metadata by PMID |

**Features:**
- 35+ million citations from MEDLINE and life science journals
- MeSH term support
- No authentication required

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Search articles | `pubmed/search` |
| Get abstract | `pubmed/get` |

---

### bioRxiv / medRxiv (`biorxiv`)
> Preprints via the official bioRxiv API

| Tool | Description |
|------|-------------|
| `get_recent_preprints` | Recent preprints |
| `get_preprints_by_date` | Preprints by date range |
| `get_preprint_by_doi` | Preprint by DOI |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Recent preprints | `biorxiv/get_recent_preprints` |
| Date range | `biorxiv/get_preprints_by_date` |
| DOI lookup | `biorxiv/get_preprint_by_doi` |

---

### Google Scholar (`google_scholar`)
> Scholar search via scraping (unofficial)

| Tool | Description |
|------|-------------|
| `search_papers` | Search papers |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Search papers | `google_scholar/search_papers` |

---

### Semantic Scholar
> Academic papers with citation graphs

| Tool | Description |
|------|-------------|
| `search_papers` | Search papers |
| `get_paper_details` | Paper details by paper_id |
| `get_related_papers` | Related papers by paper_id |

**Features:**
- Citation and reference graphs
- Influence and citation velocity metrics
- Free API (no auth required)

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Search papers | `semantic_scholar/search_papers` |
| Paper details | `semantic_scholar/get_paper_details` |
| Related papers | `semantic_scholar/get_related_papers` |

---

### SciHub (Open Access)
> Best-effort open-access lookup by DOI (does not bypass paywalls)

| Tool | Description |
|------|-------------|
| `get` / `get_paper` | Lookup open-access locations by DOI |

**Features:**
- Find PDF URLs for open-access versions of papers
- Returns metadata: title, authors, year, journal
- Uses OpenAlex (free, no auth) with optional Unpaywall fallback
- Does NOT bypass paywalls—only returns legally available copies

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Open-access lookup by DOI | `scihub/get` |

**Configuration (Optional):**
```bash
# For better results, set Unpaywall email
export UNPAYWALL_EMAIL="your.email@example.com"
# Or configure interactively
rzn-tools setup scihub
```

**Example:**
```bash
rzn-tools scihub paper --doi "10.1371/journal.pone.0000308"
rzn-tools scihub paper --doi "10.1038/nature12373" --output json
```

**Response Fields:**
- `doi` - The queried DOI
- `pdf_url` - Direct PDF link (if found)
- `title`, `authors`, `year`, `journal` - Paper metadata
- `success` - `true` if open-access PDF was found
- `message` - Status/source info

[Full Documentation →](connectors/scihub.md)

---

## Web Search

### Search APIs

| Connector | Description | Auth Required |
|-----------|-------------|---------------|
| `serper-search` | Google Search via Serper | API Key |
| `serpapi-search` | Multi-engine via SerpAPI | API Key |
| `tavily-search` | AI-optimized search | API Key |
| `exa-search` | Neural search | API Key |
| `firecrawl-search` | Web crawling & search | API Key |

**Common Features:**
- Structured search results with snippets
- Pagination support
- Domain filtering

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Web search via Serper | `serper-search/search` |
| Web search via SerpAPI | `serpapi-search/search` |
| Web/news search via Tavily | `tavily-search/search` |
| Semantic search via Exa | `exa-search/search` |
| Search + scrape via Firecrawl | `firecrawl-search/search` |
| Exa extra tools | `exa-search/get_contents`, `exa-search/find_similar`, `exa-search/answer`, `exa-search/research` |

---

### Parallel Search (`parallel_search`)
> Parallel multi-query search and scheduled monitoring

| Tool | Description |
|------|-------------|
| `search` | Parallel web search |
| `create_monitor` | Create a monitor |
| `list_monitors` | List monitors |
| `get_monitor_events` | Monitor events |
| `cancel_monitor` | Cancel monitor |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Parallel search | `parallel_search/search` |
| Create monitor | `parallel_search/create_monitor` |
| List monitors | `parallel_search/list_monitors` |
| Monitor events | `parallel_search/get_monitor_events` |
| Cancel monitor | `parallel_search/cancel_monitor` |

---

## AI-Powered Search

These connectors use LLM providers' native web search capabilities:

### OpenAI Search (`openai-search`)
> Web search via OpenAI Responses API

| Tool | Description |
|------|-------------|
| `search` | Grounded web search with AI synthesis |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Grounded web search | `openai-search/search` |

**Auth:** `OPENAI_API_KEY`

---

### Claude Web Search (`anthropic-search`)
> Web search via Anthropic's Claude

| Tool | Description |
|------|-------------|
| `search` | Grounded search with citations |

**Parameters:**
- `query` - Search query
- `max_results` - Result limit
- `allowed_domains` / `blocked_domains` - Domain filtering
- `date_range` - Time filtering

**Auth:** `ANTHROPIC_API_KEY`

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Grounded web search | `anthropic-search/search` |

---

### Gemini Search (`gemini-search`)
> Google Search grounding via Gemini

| Tool | Description |
|------|-------------|
| `search` | Search with Google's latest index |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Grounded web search | `gemini-search/search` |

**Auth:** `GOOGLE_API_KEY`

---

### Perplexity Search (`perplexity-search`)
> Real-time web search with AI synthesis

| Tool | Description |
|------|-------------|
| `search` | Search with real-time results |

**Auth:** `PERPLEXITY_API_KEY`

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Grounded web search | `perplexity-search/search` |

---

### Additional AI Search Providers

| Connector | Description | Auth |
|-----------|-------------|------|
| `tavily-search` | Fast search with summaries | `TAVILY_API_KEY` |
| `exa-search` | Semantic/neural search | `EXA_API_KEY` |
| `firecrawl-search` | Web scraping + search | `FIRECRAWL_API_KEY` |
| `xai-search` | xAI Grok web/X search (Responses API tools) | `XAI_API_KEY` |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Grounded web/X search via xAI | `xai-search/search` |

`xai-search` stays separate from `x`; it only covers `web_search` and `x_search`.

Common `xai-search/search` knobs:

- `source=web|x`
- `include_domains` / `exclude_domains`
- `allowed_x_handles` / `excluded_x_handles`
- `from_date` / `to_date`
- `enable_image_understanding`
- `enable_video_understanding`

---

## Productivity

### CalDAV (`caldav`)
> Calendar discovery plus event read/write on CalDAV providers (iCloud, Fastmail, Nextcloud, Radicale)

| Tool | Description |
|------|-------------|
| `list_calendars` | Discover calendars for the authenticated account |
| `list` | List events from a calendar/time window (supports cursor + normalized output) |
| `get` | Fetch a single event by `item_ref` or URL |
| `create` | Create an event (structured fields or raw VCALENDAR payload) |
| `update` | Update an event by `item_ref`/URL |
| `delete` | Delete an event by `item_ref`/URL |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List calendars | `caldav/list_calendars` |
| List events | `caldav/list` |
| Get one event | `caldav/get` |
| Create event | `caldav/create` |
| Update event | `caldav/update` |
| Delete event | `caldav/delete` |

**Auth:** Username/password (often app-specific) or bearer token
**Notes:** Configure `base_url` plus auth credentials. Set `calendar_url` when you want a fixed default calendar.
**Detailed setup:** `docs/connectors/caldav.md` (provider-specific iCloud/Fastmail/Nextcloud/Radicale instructions)

---

### Slack
> Workspace messages, channels, and files

| Tool | Description |
|------|-------------|
| `test_auth` | Verify Slack connection |
| `list_channels` | List all workspace channels |
| `list_messages` | Get messages from a channel |
| `get_thread` | Get thread replies |
| `search_messages` | Search across workspace |
| `list_files` | List files in a channel |
| `get_thread_by_permalink` | Get thread by Slack URL |

**Auth:** Bot Token (`xoxb-...`)

```bash
rzn-tools setup slack
rzn-tools config set slack --value "xoxb-your-token"
```

**Required Scopes:** `channels:read`, `channels:history`, `users:read`, `files:read`, `search:read`

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List channels | `slack/list_channels` |
| Recent channel messages | `slack/list_messages` |
| Thread replies | `slack/get_thread` |
| Search messages | `slack/search_messages` |
| List files | `slack/list_files` |
| Thread from permalink | `slack/get_thread_by_permalink` |

---

### Discord (`discord`)
> Servers, channels, and messages (bot token)

| Tool | Description |
|------|-------------|
| `list_servers` | List servers |
| `get_server_info` | Server details |
| `list_channels` | List channels |
| `read_messages` | Read channel messages |
| `search_messages` | Search channel messages |
| `send_message` | Send message |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List servers | `discord/list_servers` |
| Server info | `discord/get_server_info` |
| List channels | `discord/list_channels` |
| Read messages | `discord/read_messages` |
| Search messages | `discord/search_messages` |
| Send message | `discord/send_message` |

---

### WhatsApp (`whatsapp`)
> WhatsApp messages, contacts, and groups (via local WuzAPI; unofficial client)

| Tool | Description |
|------|-------------|
| `connect` | Connect and return QR (if needed) |
| `status` | Session status |
| `disconnect` | Disconnect websocket (keep session) |
| `logout` | Logout (QR required next time) |
| `send_text` | Send a text message |
| `send_media` | Send image/audio/video/document/sticker from local file |
| `send_location` | Send a location pin |
| `list_contacts` | List synced contacts |
| `list_groups` | List joined groups |
| `get_group_info` | Get group details |
| `get_messages` | Read local chat history (requires history enabled) |
| `set_history` | Enable/disable local history capture |

**Notes:** Uses an unofficial WhatsApp multi-device client (`whatsmeow` via WuzAPI). This may violate WhatsApp ToS and can risk bans.

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Connect / QR | `whatsapp/connect` |
| Status | `whatsapp/status` |
| Send text | `whatsapp/send_text` |
| Read history | `whatsapp/get_messages` |

**Setup (quick):** See `docs/connectors/whatsapp.md` for the full guide.

1. Start WuzAPI locally (HTTP mode).
2. Provide credentials to rzn-tools (recommended via env vars in your MCP server config):

```bash
export WUZAPI_BASE_URL="http://127.0.0.1:8080"  # default
export WUZAPI_TOKEN="your_wuzapi_user_token"    # required for normal endpoints (/session/*, /chat/*, ...)
```

3. Call `whatsapp/health`, then `whatsapp/connect` and scan the QR code.
4. Use `whatsapp/status` to confirm, then `whatsapp/send_text`.

**Common config fields / env vars:**
- `base_url` / `WUZAPI_BASE_URL` (default `http://localhost:8080`)
- `token` / `WUZAPI_TOKEN` (required; WuzAPI user token)
- `admin_token` / `WUZAPI_ADMIN_TOKEN` (used for WuzAPI admin endpoints; also passed when auto-starting)
- `wuzapi_path` / `WUZAPI_PATH` (optional; allows rzn-tools to auto-start WuzAPI)
- `data_dir` / `WUZAPI_DATA_DIR` (optional; when auto-starting; defaults to `~/.config/rzn-tools/wuzapi`)

---

### Telegram (`telegram`)
> Dialogs and messages via MTProto (user session)

| Tool | Description |
|------|-------------|
| `status` | Session authorized? |
| `start_login` | Send login code to phone |
| `complete_login` | Complete login with code (+ optional 2FA password) |
| `resolve_username` | Resolve `@username` → `peer_ref` |
| `list_dialogs` | List dialogs with `peer_ref` |
| `get_messages` | Get recent dialog messages |
| `search_messages` | Search messages within a dialog |
| `send_message` | Send a message |

**Auth:** `api_id` + `api_hash` (from https://my.telegram.org/apps) + local `session_file`.

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Send login code | `telegram/start_login` |
| Complete login | `telegram/complete_login` |
| List dialogs | `telegram/list_dialogs` |
| Read messages | `telegram/get_messages` |

**Setup (quick):** See `docs/connectors/telegram.md` for the full guide.

1. Create an app at https://my.telegram.org/apps and copy `api_id` + `api_hash`.
2. Provide credentials to rzn-tools (recommended via env vars in your MCP server config):

```bash
export TG_ID="123456"
export TG_HASH="0123456789abcdef0123456789abcdef"
export TG_SESSION_FILE="$HOME/.config/rzn-tools/telegram.session"  # optional
```

3. Login (from your MCP client):
   - `telegram/start_login` → `{ "phone": "+15551234567" }`
   - `telegram/complete_login` → `{ "code": "12345", "password": "..." }` (password only for 2FA)
4. Use `telegram/list_dialogs` to obtain `peer_ref` objects for `telegram/get_messages` and `telegram/send_message`.

---

### GitHub
> Repositories, issues, PRs, and code search

| Tool | Description |
|------|-------------|
| `list_issues` | List issues with filters |
| `get_issue` | Get issue details |
| `list_pull_requests` | List pull requests |
| `get_pull_request` | Get PR details |
| `get_pull_diff` | Get PR diff (size-capped) |
| `code_search` | Search code across GitHub |
| `get_file` | Get file contents |

**Auth:** Personal Access Token

```bash
rzn-tools setup github
rzn-tools config set github --value "ghp_your_token"
```

**Required Scopes:** `repo` (read), `read:org`

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List issues | `github/list_issues` |
| Issue details | `github/get_issue` |
| List PRs | `github/list_pull_requests` |
| PR details | `github/get_pull_request` |
| PR diff | `github/get_pull_diff` |
| Code search | `github/code_search` |
| File contents | `github/get_file` |

---

### Atlassian
> Jira issues and Confluence pages

| Tool | Description |
|------|-------------|
| `test_auth` | Validate Jira/Confluence auth |
| `jira_search_issues` | Search Jira issues (JQL) |
| `jira_get_issue` | Get Jira issue details |
| `conf_search_pages` | Search Confluence pages (CQL) |
| `conf_get_page` | Get Confluence page |

**Auth:** API Token + Email

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Jira search (JQL) | `atlassian/jira_search_issues` |
| Jira issue details | `atlassian/jira_get_issue` |
| Confluence search | `atlassian/conf_search_pages` |
| Confluence page | `atlassian/conf_get_page` |

---

## Google Workspace

### Gmail (`google-gmail`)
| Tool | Description |
|------|-------------|
| `list_messages` | List messages (q filter) |
| `get_message` | Get message by id |
| `get_thread` | Get thread by id |
| `decode_message_raw` | Decode raw message |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List/search messages | `google-gmail/list_messages` |
| Message details | `google-gmail/get_message` |
| Thread details | `google-gmail/get_thread` |
| Decode raw message | `google-gmail/decode_message_raw` |

**Notes:** Requires explicit user permission.

### Calendar (`google-calendar`)
| Tool | Description |
|------|-------------|
| `list_events` | List events |
| `create_event` | Create event |
| `update_event` | Update event |
| `delete_event` | Delete event |
| `sync_events` | Incremental sync |
| `watch_events` | Start webhook (if enabled) |
| `stop_channel` | Stop webhook |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List events | `google-calendar/list_events` |
| Create event | `google-calendar/create_event` |
| Update event | `google-calendar/update_event` |
| Delete event | `google-calendar/delete_event` |
| Incremental sync | `google-calendar/sync_events` |

**Notes:** Requires explicit user permission.

### Drive (`google-drive`)
| Tool | Description |
|------|-------------|
| `list_files` | List files and folders |
| `get_file` | Get file metadata |
| `download_file` | Download file (base64) |
| `export_file` | Export Docs/Sheets/Slides |
| `upload_file` | Upload file (base64) |
| `upload_file_resumable` | Resumable upload |
| `find_and_export` | Find and export Doc/Sheet/Slide |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List/search files | `google-drive/list_files` |
| File metadata | `google-drive/get_file` |
| Download content | `google-drive/download_file` |
| Export Doc/Sheet/Slide | `google-drive/export_file` |
| Upload file | `google-drive/upload_file` |
| Resumable upload | `google-drive/upload_file_resumable` |
| Find and export | `google-drive/find_and_export` |

**Notes:** Requires explicit user permission.

### Contacts (`google-people`)
| Tool | Description |
|------|-------------|
| `list_connections` | List contacts |
| `get_person` | Get contact by resourceName |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List contacts | `google-people/list_connections` |
| Contact details | `google-people/get_person` |

**Notes:** Requires explicit user permission.

**Auth:** OAuth 2.0 or Service Account

---

## SEO & Search Console

### Google Search Console (`google-search-console`)
> SEO performance, sitemaps, and URL inspection

For setup and calling patterns, see `docs/connectors/google_search_console.md`.

| Tool | Description |
|------|-------------|
| `list_sites` | List Search Console properties |
| `get_site` | Get property details + permission level |
| `search_analytics` | Clicks/impressions/CTR/position queries |
| `list_sitemaps` | List submitted sitemaps |
| `get_sitemap` | Sitemap details |
| `submit_sitemap` | Submit a sitemap |
| `delete_sitemap` | Delete a sitemap |
| `inspect_url` | URL Inspection (index/crawl/rich results) |
| `query_builder` | Preset query args for `search_analytics` |

**Auth:** Google OAuth device flow (`rzn-tools setup google-search-console`)

---

### Bing Webmaster Tools (`bing-webmaster-tools`)
> Bing search metrics + diagnostics + URL submission + IndexNow (optional)

For setup and calling patterns, see `docs/connectors/bing_webmaster_tools.md`.

| Tool | Description |
|------|-------------|
| `list_sites` | List sites in your Bing account |
| `get_rank_and_traffic_stats` | Overall site metrics |
| `get_query_stats` | Query performance |
| `get_page_stats` | Page performance |
| `get_crawl_stats` | Crawl stats |
| `get_crawl_issues` | Crawl issues |
| `submit_url` | Submit a URL for indexing (Bing API) |
| `submit_url_batch` | Batch URL submission (Bing API) |
| `indexnow_submit_url` | Submit a URL via IndexNow (optional) |
| `indexnow_submit_url_batch` | Batch submit via IndexNow (optional) |

**Auth:** Bing API key (`BING_WEBMASTER_API_KEY`) and/or IndexNow key (`INDEXNOW_KEY`)

---

## Microsoft 365

### Microsoft Graph (`microsoft`)
> Unified API for Microsoft 365 services

| Tool | Description |
|------|-------------|
| `list_messages` | List Outlook messages |
| `get_message` | Get message by ID |
| `list_events` | List calendar events |
| `send_mail` | Send email |
| `create_draft` | Create draft email |
| `upload_attachment_large` | Upload attachment (base64) |
| `upload_attachment_large_from_path` | Upload attachment from file |
| `send_draft` | Send draft |
| `auth_start` | Start device auth |
| `auth_poll` | Poll device auth |

**Auth:** Azure AD OAuth

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List messages | `microsoft/list_messages` |
| Message details | `microsoft/get_message` |
| List events | `microsoft/list_events` |
| Send mail | `microsoft/send_mail` |
| Draft + attachment | `microsoft/create_draft`, `microsoft/upload_attachment_large` |
| Send draft | `microsoft/send_draft` |

**Notes:** Requires explicit user permission.

---

## SMTP

### SMTP (`smtp`)
> Outbound email sending via standard SMTP providers

| Tool | Description |
|------|-------------|
| `send_mail` | Send an outbound email (supports text + optional HTML) |
| `test_connection` | Verify SMTP connectivity/authentication with NOOP |

**Auth:** SMTP credentials (`host`, `port`, `username`, `password`, `security`)

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Test SMTP login | `smtp/test_connection` |
| Send email | `smtp/send_mail` |

**Notes:** Requires explicit user permission. Recommended to use app passwords for Gmail/iCloud/Outlook.

---

## Feeds

### RSS (`rss`)
> RSS/Atom/JSON feeds

| Tool | Description |
|------|-------------|
| `get_feed` | Fetch feed + entries |
| `list_entries` | List entries |
| `search_feed` | Search entries |
| `discover_feeds` | Discover feeds on a webpage |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Fetch feed | `rss/get_feed` |
| List entries | `rss/list_entries` |
| Search entries | `rss/search_feed` |
| Discover feeds | `rss/discover_feeds` |

---

## Local System

### Local Files (`localfs`)
> Local filesystem indexing and extraction

| Tool | Description |
|------|-------------|
| `list_files` | List files |
| `get_file_info` | File metadata |
| `extract_text` | Extract file text |
| `get_structure` | Document structure |
| `get_section` | Get section |
| `search_content` | Search within file |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| List files | `localfs/list_files` |
| File metadata | `localfs/get_file_info` |
| Extract text | `localfs/extract_text` |
| Document structure | `localfs/get_structure` |
| Get section | `localfs/get_section` |
| Search content | `localfs/search_content` |

---

### Spotlight (`spotlight`)
> macOS Spotlight index search

| Tool | Description |
|------|-------------|
| `search_content` | Full-text search |
| `search_by_name` | Search by name |
| `search_by_kind` | Search by kind |
| `search_recent` | Recently modified |
| `get_metadata` | File metadata |
| `raw_query` | Raw mdfind query |

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Full-text search | `spotlight/search_content` |
| Search by name | `spotlight/search_by_name` |
| Search by kind | `spotlight/search_by_kind` |
| Recent files | `spotlight/search_recent` |
| File metadata | `spotlight/get_metadata` |
| Raw query | `spotlight/raw_query` |

---

## Web Scraping

### Web (`web`)
> Generic web content extraction

| Tool | Description |
|------|-------------|
| `scrape_url` | Extract text content from URL |
| `scrape_with_config` | Advanced scraping with selectors |

**Features:**
- Clean text extraction
- Custom CSS selectors
- No authentication required

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Extract text from URL | `web/scrape_url` |
| Custom selectors | `web/scrape_with_config` |

---

## Reference

### Wikipedia
> Encyclopedia articles in multiple languages

| Tool | Description |
|------|-------------|
| `search` | Search Wikipedia |
| `get_article` | Get article content |
| `geosearch` | Find articles by location |

**Features:**
- Multi-language support
- Geographic search by coordinates
- No authentication required

**Task → Tool (MCP name):**
| Task | Tool |
|------|------|
| Keyword search | `wikipedia/search` |
| Article content | `wikipedia/get_article` |
| Geo search | `wikipedia/geosearch` |

---

## Authentication Quick Reference

### No Authentication Required
```
arxiv, hackernews, pubmed, scihub, semantic_scholar, web, wikipedia, youtube*
```
*YouTube works without auth but may have rate limits

### Environment Variables
```bash
# AI Search
export OPENAI_API_KEY="..."
export ANTHROPIC_API_KEY="..."
export PERPLEXITY_API_KEY="..."
export TAVILY_API_KEY="..."

# Productivity
export SLACK_TOKEN="xoxb-..."
export GITHUB_TOKEN="ghp_..."

# Social
export REDDIT_CLIENT_ID="..."
export REDDIT_CLIENT_SECRET="..."
```

### CLI Configuration
```bash
rzn-tools setup                      # Interactive wizard
rzn-tools setup <connector>          # Configure specific connector
rzn-tools config set <connector> --value "token"
rzn-tools config test <connector>    # Verify authentication
```

### Config File Location
- **macOS/Linux:** `~/.config/rzn-tools/auth.json`
- **Windows:** `%APPDATA%\rzn-tools\auth.json`

---

## Feature Flags

Build with specific connectors to minimize binary size:

```bash
# Minimal (no connectors)
cargo build --release -p rzn_tools_cli --no-default-features

# Specific connectors
cargo build --release -p rzn_tools_cli --features "youtube,hackernews,arxiv"

# All connectors
cargo build --release -p rzn_tools_cli --features full

# AI search providers
cargo build --release -p rzn_tools_cli --features "openai-search,anthropic-search"
```

---

## Need Help?

```bash
rzn-tools --help                     # General help
rzn-tools tools <connector>          # Show connector tools
rzn-tools connectors                 # List all connectors
```

[GitHub Issues](https://github.com/srv1n/rzn-tools/issues) | [Installation Guide](../INSTALLATION.md)
