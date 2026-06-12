# Downstream Integration Guide (Tool API + Migration Notes)

This document is for teams integrating rzn-tools **downstream** (custom hosts, agent runtimes, internal
platforms) that call rzn-tools tools via MCP or via the CLI wrappers.

It describes the **canonical tool API** conventions and provides a migration checklist for recent
connector surface changes (focused on minimizing ambiguity and tool-selection errors for agents).

## TL;DR

- Prefer a **small canonical surface** per connector:
  - `search` — keyword search/discovery
  - `get` — fetch a specific item by URL/ID
  - `list` — browse a feed/collection (subreddit feed, folder listing, etc.)
- Prefer canonical parameter names:
  - `limit` for “how many results”
  - `response_format` (`concise` default, `detailed` for full metadata)
- For **ingestion/indexing pipelines**, prefer requesting a normalized, chunk-ready output (opt-in):
  - `output_format: "normalized_v1"`
  - See: `docs/integrations/INGEST_CONTRACT_V1.md`
- Legacy tool names are often still accepted for backwards compatibility, but may be **hidden from
  `list_tools()`** to keep the action space small for agents.

## Principles (Why this exists)

rzn-tools is designed to be “tools for agents” friendly:

- Tool names should be easy to pick correctly with minimal context.
- Tool descriptions should differentiate *subtle* choices (“feed listing” vs “keyword search”).
- Tool surfaces should avoid explosion (do not create one tool per minor variant).

When a connector previously exposed many overlapping tools (e.g., “top/new/hot” as separate tools),
we consolidate into a canonical interface with **mode/sort/time parameters** instead.

## How downstream callers should integrate

### 1) Discover tools dynamically (preferred)

Do not hardcode tool names when possible.

- Call `tools/list` on startup or per-session.
- Choose tools by name, but tolerate connector upgrades by falling back to legacy names if needed.

### 2) Call tools with canonical names/params (stable contract)

Canonical names are intended to remain stable:

- `connector/search`
- `connector/get`
- `connector/list` (only if the connector has a “feed” or “collection” concept)

Canonical params:

- `limit`: integer count of results (search/list)
- `response_format`: `concise|detailed`

### 3) Treat legacy names as best-effort compatibility only

Legacy names may remain callable for scripts and older clients, but:

- They may not appear in `list_tools()` anymore.
- They may not be referenced by the Smart Resolver.

## Connector migration notes

### Reddit (connector: `reddit`)

Canonical tools:

- `reddit/list` — subreddit feed browsing (hot/new/top)
- `reddit/search` — keyword search (optionally scoped to a subreddit)
- `reddit/get` — fetch a post + comments
- `reddit/media` — resolve ordered media URLs for a post

Legacy tool names remain callable (not listed):

- `reddit/get_subreddit_top_posts`, `reddit/get_subreddit_hot_posts`, `reddit/get_subreddit_new_posts`
- `reddit/search_reddit`
- `reddit/get_post_details`

Recommended calls:

```json
{"method":"tools/call","params":{"name":"reddit/list","arguments":{"subreddit":"rust","sort":"top","time":"week","limit":10}}}
{"method":"tools/call","params":{"name":"reddit/search","arguments":{"query":"async await","subreddit":"rust","sort":"top","time":"month","limit":10}}}
{"method":"tools/call","params":{"name":"reddit/get","arguments":{"post_url":"https://www.reddit.com/r/rust/comments/abc123/example_post","comment_limit":25,"comment_sort":"best"}}}
{"method":"tools/call","params":{"name":"reddit/media","arguments":{"item_ref":"reddit:post:abc123"}}}
```

### YouTube (connector: `youtube`)

Canonical tools:

- `youtube/search` — search videos/playlists/channels (via `search_type`)
- `youtube/get` — video metadata + transcript for videos; ordered `entries[]` enumeration for playlist/channel inputs
- `youtube/list` — list recent uploads from a channel or playlist (for “last N videos” workflows)
- `youtube/resolve_channel` — resolve a channel name/handle/url to a stable UC... channel ID, with ranked candidates

Legacy tool names remain callable (not listed):

- `youtube/search_videos`
- `youtube/get_video_details`

Recommended calls:

```json
{"method":"tools/call","params":{"name":"youtube/search","arguments":{"query":"rust programming","limit":5,"search_type":"video"}}}
{"method":"tools/call","params":{"name":"youtube/get","arguments":{"video_id":"dQw4w9WgXcQ","response_format":"concise"}}}
{"method":"tools/call","params":{"name":"youtube/get","arguments":{"url":"https://www.youtube.com/playlist?list=PL590L5WQmH8fJ54F9CrK3KrhE6i2yWm9n"}}}
{"method":"tools/call","params":{"name":"youtube/list","arguments":{"source":"channel","channel":"@hubermanlab","limit":5,"published_within_days":7}}}
{"method":"tools/call","params":{"name":"youtube/resolve_channel","arguments":{"query":"Andrew Huberman","limit":5,"prefer_verified":true}}}
```

Common workflows

**A) “Last 5 videos from Andrew Huberman’s official channel”**

1) Resolve a stable channel ID (UC…):

```json
{"method":"tools/call","params":{"name":"youtube/resolve_channel","arguments":{"query":"Andrew Huberman","limit":5,"prefer_verified":true}}}
```

2) Take `recommended.channel_id` (or ask the user to pick from `candidates`) and list uploads:

```json
{"method":"tools/call","params":{"name":"youtube/list","arguments":{"source":"channel","channel":"UC...","limit":5}}}
```

3) For each returned video ID, call `youtube/get` and summarize from `transcript`/`chapters`.

**B) “Enumerate a whole playlist, then fetch transcripts”**

```bash
rzn-tools get youtube "$PLAYLIST_URL" --output json \
  | jq -r '.data.entries[].id' \
  | while read -r id; do
      rzn-tools get youtube "$id" --field transcript --output text > "raw/$id.txt"
    done
```

**C) “Last 5 videos from the last week (official channel)”**

Same as (A), but add a time filter:

```json
{"method":"tools/call","params":{"name":"youtube/list","arguments":{"source":"channel","channel":"UC...","limit":5,"published_within_days":7}}}
```

Current implementation notes

- `youtube/list` is implemented with native YouTube page parsing plus Innertube `browse`
  continuation pagination. It supports:
  - `source="channel"` with `channel="@handle"|channel_url|channel_id`
  - `source="playlist"` with `playlist=playlist_url|playlist_id`
  - raw output includes ordered `entries[]` and a back-compat `videos[]` alias
  - each entry includes `id`, `title`, `index`, `url`, and best-effort channel/playlist metadata
  - omitted `limit` to enumerate until YouTube stops returning continuation pages
  - optional `limit` for last-N channel uploads or first-N playlist items
  - optional time filters: `published_within_days` or `published_after` (RFC3339)
- `youtube/get` delegates playlist IDs/URLs and channel handles/URLs to the same enumeration path.
- `rzn-tools fetch "https://www.youtube.com/playlist?list=..."` and channel URLs route to
  `youtube/list`, not the generic web scraper.
- `youtube/resolve_channel` ranks candidates using token overlap + (optional) verified preference + subscriber count.

Important: “official channel” is a best-effort heuristic

`youtube/resolve_channel` helps reduce ambiguity, but it is not an authoritative verification API.
Downstream hosts should:

- Present `candidates[]` to the user in interactive contexts.
- Prefer a pinned UC… channel ID in configuration once chosen.

### arXiv (connector: `arxiv`)

Canonical tools:

- `arxiv/search` — search papers (`limit` is canonical; `max_results` still accepted)
- `arxiv/get` — paper metadata by `paper_id`

Legacy tool names remain callable (not listed):

- `arxiv/search_papers`
- `arxiv/get_paper_details`
- `arxiv/get_pdf_url` / `arxiv/get_paper_pdf` (best-effort legacy)

### PubMed (connector: `pubmed`)

Canonical tools:

- `pubmed/search`
- `pubmed/get` — abstract + metadata by `pmid`

Legacy tool names remain callable (not listed):

- `pubmed/get_abstract`

### Sci-Hub (connector: `scihub`)

Canonical tools:

- `scihub/get` — DOI → best-effort open-access PDF URL metadata (no paywall bypass)

Legacy tool names remain callable (not listed):

- `scihub/get_paper`

### Spotlight (connector: `spotlight`, macOS)

Canonical tools:

- `spotlight/search` — use `mode=content|name|kind|recent|raw`
- `spotlight/get_metadata`

Legacy tool names remain callable (not listed):

- `spotlight/search_content`, `spotlight/search_by_name`, `spotlight/search_by_kind`,
  `spotlight/search_recent`, `spotlight/raw_query`

## Smart Resolver expectations

The Smart Resolver (used by `rzn-tools fetch`) generally routes to canonical tools:

- YouTube URLs/IDs → `youtube/get`
- Reddit post URL → `reddit/get`
- `r/<subreddit>` → `reddit/list`

If your downstream integration relies on resolver output, expect the `tool` field to be the
canonical name.

## CLI compatibility notes

The CLI continues to expose human-friendly subcommands (e.g., `rzn-tools reddit top ...`) while mapping
them to canonical tool calls internally.

Downstream teams should prefer MCP tools over parsing CLI output.
