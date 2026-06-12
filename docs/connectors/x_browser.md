# X (Twitter) browser-cookies connector (`x-browser`)

This document is written for **LLM tool-calling** (MCP) and **CLI** usage. It focuses on the
practical details a model (or a human) needs: **how to call each tool**, **how pagination works**,
and **how to pass time filters correctly**.

If you have X API access and want the **official X API v2** tools, use connector `x` and see
`docs/connectors/x.md`.

## Quick reference

Tools (scraper-based):

- `x-browser/get_profile` — profile metadata for a username
- `x-browser/search_tweets` — keyword search with pagination + filtering + sorting
- `x-browser/get_tweet` — tweet details by ID (also used by URL auto-resolve)
- `x-browser/get_thread` — conversation/thread for a focal tweet ID
- `x-browser/get_user_tweets` — tweets-only feed for a user (no replies)
- `x-browser/fetch_tweets_and_replies` — activity feed (tweets + replies)
- `x-browser/search_profiles` — account discovery with pagination
- `x-browser/get_followers` — followers list with pagination

Account-level tools (require explicit permission):

- `x-browser/get_home_timeline`
- `x-browser/get_direct_message_conversations`
- `x-browser/send_direct_message`

## Authentication

Recommended: browser-cookie auth.

```bash
rzn-tools setup x-browser
rzn-tools config set x-browser --browser chrome
```

Credential auth is also supported (`username`, `password`) with optional `email` and `2fa_secret`.

## Date/time filtering (very important)

`x-browser/search_tweets`, `x-browser/get_user_tweets`, and `x-browser/get_thread` support time filtering.

### Accepted formats

- **RFC3339**: `2026-02-24T13:45:00Z`, `2026-02-24T13:45:00-05:00`
- **Date-only**: `YYYY-MM-DD` (interpreted as **UTC**)

Date-only expansion:

- `start_time: "2026-02-24"` → `2026-02-24T00:00:00Z`
- `end_time: "2026-02-24"` → `2026-02-24T23:59:59Z`

### Which fields to use

`x-browser/search_tweets` supports *two layers* of time filtering:

1) **Query operators** (server-side):
   - `since` (YYYY-MM-DD) is appended to the query as `since:YYYY-MM-DD`
   - `until` (YYYY-MM-DD) is appended to the query as `until:YYYY-MM-DD`

2) **Local post-filter** (client-side):
   - `start_time` and `end_time` are applied to returned tweets using their parsed timestamps.

Rules:

- If you pass `start_time`/`end_time`, those take effect as the precise filter.
- If you pass `since`/`until` but omit `start_time`/`end_time`, rzn-tools will also use them as a
  local time window:
  - `since` → `start_time = YYYY-MM-DDT00:00:00Z`
  - `until` → `end_time   = YYYY-MM-DDT23:59:59Z`
- If your `query` already contains `since:` or `until:`, you should avoid also passing `since`/`until`
  to prevent confusion (rzn-tools will only append if the query doesn’t already contain them).

### Sorting vs filtering

You can request local sorting in addition to X’s native search mode:

- `sort_by="time"` sorts by tweet timestamp.
- `sort_by="engagement"` sorts by a local score:
  `likes + retweets + replies + quotes`.

Use `order="desc"` for “newest/highest first” and `order="asc"` for “oldest/lowest first”.

## Pagination (cursor-based)

Some tools return a `next_cursor` field you can pass back in `cursor`:

- `x-browser/search_tweets`
- `x-browser/search_profiles`
- `x-browser/get_user_tweets`
- `x-browser/get_followers`
- `x-browser/fetch_tweets_and_replies`

Pattern:

1) Call tool without `cursor`
2) Read `next_cursor`
3) Call again with `cursor=next_cursor`

## Tool calling (MCP) — examples

All examples are **arguments objects** (what you put into MCP `call_tool` as `arguments`).

### `x-browser/search_tweets`

Use when you need “find relevant posts about X”.

Engagement-sorted results from a specific date range:

```json
{
  "query": "rust lang:en",
  "limit": 30,
  "mode": "top",
  "since": "2026-02-01",
  "until": "2026-02-24",
  "exclude_retweets": true,
  "exclude_replies": true,
  "min_likes": 10,
  "sort_by": "engagement",
  "order": "desc"
}
```

Precise time window (RFC3339) + pagination:

```json
{
  "query": "\"agentic\" lang:en",
  "limit": 20,
  "mode": "latest",
  "start_time": "2026-02-24T00:00:00Z",
  "end_time": "2026-02-24T23:59:59Z",
  "cursor": "<paste next_cursor here>"
}
```

### `x-browser/get_thread`

Use when you have a tweet id and need the thread/conversation context.

```json
{
  "tweet_id": "1234567890123456789",
  "limit": 200,
  "sort_by": "time",
  "order": "asc"
}
```

### `x-browser/get_user_tweets`

Use when you want a clean “recent tweets” feed (no replies). `username` can be a handle or a
numeric `user_id`.

```json
{
  "username": "rustlang",
  "limit": 100,
  "exclude_retweets": true,
  "start_time": "2026-02-01",
  "order": "desc"
}
```

## CLI usage — examples (`rzn-tools x-browser ...`)

The CLI maps directly onto the MCP tools above.

Search with dates + engagement sorting:

```bash
rzn-tools x-browser search --query "rust lang:en" --limit 30 --mode top \
  --since 2026-02-01 --until 2026-02-24 \
  --exclude-retweets true --exclude-replies true \
  --min-likes 10 --sort-by engagement --order desc
```

Fetch a thread in chronological order:

```bash
rzn-tools x-browser thread --tweet-id 1234567890123456789 --limit 200 --sort-by time --order asc
```

User tweets only (exclude retweets), constrained to February 2026:

```bash
rzn-tools x-browser tweets --username rustlang --limit 100 --exclude-retweets true \
  --start-time 2026-02-01 --end-time 2026-02-28 --order desc
```

## Practical LLM guidance

- Prefer `x-browser/search_tweets` for discovery, then `x-browser/get_thread` for context on the most relevant hits.
- For “what did this account say recently”, use `x-browser/get_user_tweets` (and optionally
  `exclude_retweets=true`).
- When a task asks for “between date A and date B”, pass:
  - `since`/`until` for search semantics, and
  - `start_time`/`end_time` if you need precise cutoffs (time-of-day).
