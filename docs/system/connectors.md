---
title: "Connectors"
subject: connectors
keywords: [providers, adapters, sources, features, tools]
part_of: overview
describes: [rzn_tools_core/src/connectors, rzn_tools_core/Cargo.toml, rzn_tools_core/src/lib.rs]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need the current connector catalog."
skip_when: "You need exact tool arguments. Use the live schema."
---

# Connectors

A connector adapts one provider or local source. Its name is its public namespace.

```bash
rzn-tools connectors
rzn-tools tools <connector> --output json
rzn-tools call <connector> <tool> --args '<json-object>'
```

The registry in [`rzn_tools_core/src/lib.rs`](../../rzn_tools_core/src/lib.rs) is the name authority. The connector modules are the behavior authority.

## Build sets

- The CLI default build uses `default-connectors`.
- `server-full` enables the portable server set.
- `all-connectors` adds Telegram and macOS connectors.
- `desktop-full` adds browser-profile import and `x-browser`.
- Discord is an explicit feature.

## Public web and social data

| Name | Scope | Main risk or limit |
| --- | --- | --- |
| `youtube` | Video, playlist, channel, and transcript reads | Cookies are optional. |
| `hackernews` | Story and comment search and reads | Public data only. |
| `reddit` | Listing, search, thread, media, and user reads | OAuth is optional. |
| `wikipedia` | Search, nearby search, and page reads | Public data only. |
| `rss` | RSS and Atom feed reads | JSON Feed is not supported. |
| `web` | URL fetch and scrape | Caller headers and cookies can expose private data. |
| `weather` | wttr.in weather reads | Forecast length is one to three days. |
| `x` | Official X API reads and writes | Auth and provider access levels apply. |
| `x-browser` | X browser-session reads | Browser cookies are sensitive. |
| `linkedin` | Identity, post, token, and raw API operations | An imported token is required. |
| `slack` | Channel, message, file, and user reads | The connector does not send messages. |
| `discord` | Server reads, ingest windows, and message send | A bot token is required. |
| `telegram` | Login, dialog, message, search, and send | Login state stays in the process. |
| `whatsapp` | Local WuzAPI sidecar and message operations | The sidecar is unofficial. |

## Research and search

| Name | Scope | Main risk or limit |
| --- | --- | --- |
| `arxiv` | Paper search and get | Public feed. |
| `pubmed` | Article search and get | Public HTML can change. |
| `biorxiv` | Recent, date, and DOI reads | Public API. |
| `semantic-scholar` | Paper graph search and reads | Provider rate limits apply. |
| `google-scholar` | HTML paper search | CAPTCHA can block requests. |
| `scihub` | OpenAlex and optional Unpaywall lookup | It does not bypass a paywall. |
| `openai-search` | OpenAI web search | API key required. |
| `anthropic-search` | Anthropic web search | API key required. |
| `gemini-search` | Gemini grounded search | API key required. |
| `perplexity-search` | Perplexity search | API key required. |
| `xai-search` | xAI web and X search | API key required. |
| `exa` | Search, contents, similar, answer, and research | API key required. |
| `firecrawl-search` | Web, image, and news search | API key required. |
| `serper-search` | Google result search | API key required. |
| `serpapi-search` | Search-engine results | API key required. |
| `tavily-search` | Web search and domain filters | API key required. |
| `parallel-search` | Search and monitor operations | Monitor create and cancel change remote state. |

## Work, cloud, and mail

| Name | Scope | Write behavior |
| --- | --- | --- |
| `github` | Issues, pull requests, code, repositories, and files | Read-only provider calls. |
| `atlassian` | Jira issue and Confluence page reads | Read-only provider calls. |
| `microsoft-graph` | Outlook mail and calendar | Can create drafts, send mail, and add attachments. |
| `google-drive` | File list, read, export, download, and upload | Upload changes Drive. |
| `google-gmail` | Message and thread reads | Read-only provider calls. |
| `google-calendar` | Event reads and writes | Can create, update, delete, watch, and stop. |
| `google-people` | Connection and person reads | Read-only provider calls. |
| `google-search-console` | Analytics, sitemap, and inspection | Sitemap tools change provider state. |
| `bing-webmaster-tools` | Webmaster and IndexNow operations | Several tools submit or change remote data. |
| `caldav` | Calendar reads and writes | Create, update, and delete are live. |
| `imap` | Mail reads, drafts, move, delete, and flags | Delete can expunge messages. |
| `smtp` | Connection test and send | Send is live unless `dry_run` is true. |

## Stores, ads, and markets

| Name | Scope | Write behavior |
| --- | --- | --- |
| `app-store` | Public app search, lookup, and reviews | Read-only. |
| `app-store-connect` | Apps, analytics, sales, and finance | Report-request creation changes remote state. |
| `apple-search-ads` | Apple Search Ads API v5 campaigns and reports | Campaign creation is live. |
| `play-store` | Public app page reads | Read-only. |
| `polymarket` | Public market, event, price, trade, and comment reads | Read-only. Scans are bounded. |
| `kalshi` | Public market, event, series, trade, and order-book reads | Read-only. Scans are bounded. |

Apple Search Ads uses the provider path `/api/v5`. This is an Apple API identifier.

## Local and macOS data

| Name | Scope | Main risk or limit |
| --- | --- | --- |
| `localfs` | File list, metadata, extraction, structure, section, and search | No root allowlist or path sandbox. |
| `macos` | Scripts, Shortcuts, clipboard, notification, and Finder | Commands can change local state. |
| `spotlight` | Metadata search and reads | macOS only. |
| `apple-mail` | Mail reads, draft, and send | macOS permission required. |
| `apple-contacts` | Contact reads | macOS permission required. |
| `apple-messages` | Chat reads, send, and local recipient aliases | Full Disk Access can be required. |
| `apple-notes` | Note reads and writes | macOS permission required. |
| `apple-reminders` | Reminder reads and writes | Create, update, and complete change local data. |

These Apple connectors use macOS permissions. They do not use an API key. There is no Apple Calendar connector.

## Change a connector

1. Change its module under `rzn_tools_core/src/connectors/`.
2. Update its Cargo feature.
3. Update `build_registry_enabled_only`.
4. Update resolver rules when URL handling changes.
5. Add one focused test with mocked provider data.
6. Update this page when public scope changes.

Do not add a second name for the same connector or tool.
