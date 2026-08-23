---
subject: connectors
keywords: [providers, adapters, sources, features, tools]
part_of: overview
describes: [rzn_tools_core/src/connectors, rzn_tools_core/Cargo.toml, rzn_tools_core/src/lib.rs, docs/connectors]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need the connector list, limits, auth rules, or design rules."
skip_when: "You need exact arguments for one tool. Use CLI help or MCP tools/list."
---

# Connectors

Each source adapter gives the project a small set of tools for one provider or
local data source.

A connector is one adapter for a provider or local source. The shared
interface is in `rzn_tools_core/src/lib.rs`. The implementation modules are in
`rzn_tools_core/src/connectors/`.

The live tool schema is the final source for names and arguments:

```bash
rzn-tools list
rzn-tools tools <connector> --output json
rzn-tools <connector> --help
```

Some old tool names remain callable as aliases but do not appear in the tool
catalog. Use the listed canonical name in new work.

## Build profiles

- `server-full` is the portable server set.
- `all-connectors` adds Telegram and macOS features.
- `desktop-full` adds browser-cookie import and `x-browser`.
- Discord is opt-in and is not in `server-full`.
- Apple Health is disabled and not registered.

The feature name, connector name, and CLI wrapper name can differ. For example,
the feature is `exa-search`, the connector is `exa`, and the CLI command is
`exa`.

## Public web, media, and social

| Connector | Canonical tools or role | Important limit |
| --- | --- | --- |
| `youtube` | `get`, `search`, `list`, `resolve_channel` | No required API key. Cookie and browser options are optional. |
| `hackernews` | `get_thread`, `search`, `search_recent`, `list_threads` | No user-profile tool is exposed. A proxy URL is optional. |
| `reddit` | `list`, `search`, `get`, `media`, `user` | The code has no posting or private-content tools. |
| `wikipedia` | `search`, `geosearch`, `get` | `get_article` is an old call alias. |
| `rss` | `get_feed`, `list_entries`, `search_feed`, `discover_feeds` | Parses RSS and Atom. It does not parse JSON Feed. |
| `web` | `scrape_url`, `get`, `scrape_with_config` | Can send custom browser, user-agent, and cookie data. |
| `weather` / `wttr` | `get_weather` | Uses public wttr.in. Forecast length is 1 to 3 days. |
| `x` | Official X API read and write tools | Needs bearer, OAuth 2, or OAuth 1 data. Thread lookup uses recent search. |
| `x-browser` | Browser-session X scraper | Desktop profile only. No normalized output. |
| `linkedin` | Identity, posts, raw API, and token refresh | Needs an imported LinkedIn token. |
| `slack` | Read-only channel, message, file, search, and user tools | Token comes from stored auth. Pagination is bounded. |
| `discord` | Server/channel reads, message reads/writes, and ingest windows | Needs a bot token and the Message Content intent. Search checks only recent messages. |
| `telegram` | MTProto login, dialogs, messages, search, and send | Login state must stay in the same process. Lists are capped at 500. |
| `whatsapp` | WuzAPI sidecar, chat reads, sends, history sync | Requires a local sidecar and captured history. |

## Research and search

| Connector | Canonical tools or role | Important limit |
| --- | --- | --- |
| `arxiv` | `search`, `get` | Public arXiv feed. Supports normalized output. |
| `pubmed` | `search`, `get` | Scrapes public HTML. Provider or parser failures can produce empty results. |
| `biorxiv` | `get_recent_preprints`, `get_preprints_by_date`, `get` | `get_preprint_by_doi` is only an old alias. |
| `semantic-scholar` | Search, details, related, citations, references | API key is optional in the client, but connector metadata currently marks auth as required. |
| `google-scholar` | `search_papers` | Scrapes HTML, waits before each request, and can hit CAPTCHA. |
| `scihub` | `get`, `get_paper`, `search`, `batch_get` | Uses OpenAlex and optional Unpaywall. It does not bypass paywalls. |
| `openai-search` | OpenAI Responses web search | Default model is set in the connector. |
| `anthropic-search` | Anthropic web search | Uses the Anthropic web-search tool. |
| `gemini-search` | Gemini Google Search grounding | Default model is `gemini-2.5-pro`. |
| `perplexity-search` | Perplexity online search | Key variable is `PPLX_API_KEY`. |
| `xai-search` | xAI web and X search | `sources` is an array such as `["web","x"]`. |
| `exa` | Search, contents, similar, answer, research | `research` is MCP-only in the current CLI wrapper. |
| `firecrawl-search` | Firecrawl web, image, or news search | No cursor or page argument. |
| `serper-search` | Serper Google results | No cursor or domain-filter argument. |
| `serpapi-search` | SerpAPI engine results | No cursor or date-filter argument. |
| `tavily-search` | Search with topic and domain filters | No cursor argument. |
| `parallel-search` | Search and remote monitor lifecycle | Monitor creation and cancellation change remote state. |

Most search-provider connectors return raw structured data. Do not promise
`normalized_v1` unless the tool schema lists it.

## Work, cloud, and mail

| Connector | Current scope | Important limit |
| --- | --- | --- |
| `github` | Read-only issues, pull requests, code, repositories, and files | Supports PAT and device OAuth. |
| `atlassian` | Jira issue search/get and Confluence page search/get | Basic token auth only. No create or sprint tools. |
| `microsoft-graph` | Mail and calendar reads plus mail draft/send/attachment tools | No OneDrive, Teams, or SharePoint tools. |
| `google-drive` | File list/read/download/export/upload and device auth | Default documented scope is read-only; uploads need a write scope. |
| `google-gmail` | Message and thread reads | Uses the shared Google token. |
| `google-calendar` | Event read/write/sync; optional watch tools | Writes need a write scope. Watch tools are admin-gated. |
| `google-people` | Connection and person reads | Uses contacts read-only scope. |
| `google-search-console` | Analytics, sitemap, inspection, and query builder | Sitemap operations write provider state. |
| `bing-webmaster-tools` | Bing webmaster reads/writes and IndexNow | Connector auth is API key or IndexNow key, not OAuth. |
| `caldav` | Calendar list/read/create/update/delete | Write tools change the remote calendar. |
| `imap` | Mailbox read/search plus draft/move/delete/flag tools | Delete can expunge. Dry-run is not automatic. |
| `smtp` | `send_mail`, `test_connection` | Send is live unless dry-run is requested. |

Google device tokens can be shared through `google-common`, but each API still
needs the correct scope. A successful token-presence test does not prove that a
write scope is present.

## Stores, ads, and markets

| Connector | Current scope | Important limit |
| --- | --- | --- |
| `app-store` | Public search, lookup, and reviews | No auth. |
| `app-store-connect` | App, analytics, sales, and finance tools | Accepts a P8 path or inline private key. Use Apple segment URLs only. |
| `apple-search-ads` | Campaign reads and create, reports, auth tests | Accepts inline or path-based P8 data. |
| `play-store` | Public app details | HTML parser labels are English-based; some localized fields can be missing. |
| `polymarket` | Public market, event, price, trade, and comment tools | List/search scans are bounded, not exhaustive. |
| `kalshi` | Public market, event, series, trade, and order-book tools | Search scans at most three market pages per series. |

## Local and Apple data

| Connector | Current scope | Important limit |
| --- | --- | --- |
| `localfs` | List, metadata, text extraction, structure, section, search | Direct file access. No index, watcher, root allowlist, or sandbox. |
| `macos` | AppleScript/JXA, Shortcuts, clipboard, notification, Finder | Non-macOS operations return an unsupported-platform error. |
| `spotlight` | `search` with modes, `get_metadata` | macOS only. Old search tool names are aliases. |
| `apple-mail` | Mailbox/message read, search, draft, send | macOS TCC controls access. Some old write calls are hidden aliases. |
| `apple-contacts` | Contact list/get/search | Reads personal contact data. |
| `apple-messages` | Chat/history read, send, and local aliases | History needs Full Disk Access. |
| `apple-notes` | Note read/search/create/update/append | Some folder/delete calls are hidden aliases. |
| `apple-reminders` | List and reminder read/write tools | Includes complete and delete actions. |

Apple connectors use macOS privacy permissions, not API-key credentials. There
is no Apple Calendar connector; use CalDAV for calendar access.

## Add or change a connector

1. Implement `Connector` under `rzn_tools_core/src/connectors/`.
2. Add the Cargo feature and optional dependencies.
3. Register it in `build_registry_enabled_only`.
4. Add a CLI wrapper only when typed flags add value.
5. Add one focused mocked test.
6. Add or update a short connector page.
7. Check resolver rules separately from connector URL metadata.

Keep provider calls in the connector. Keep shared behavior in core helpers.
Never put secrets or personal data in tests or docs.
