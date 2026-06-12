# RZN Integrations Ingest Contract v1 (Normalized Output + Pagination)

**Status**: Draft (intended for implementation)
**Last updated**: 2025-12-28
**Audience**: RZN Integrations core + connector authors, downstream indexers (desktop/backend), MCP integrators
**Primary motivation**: make it cheap to add new connectors while keeping downstream ingestion/indexing consistent.

---

## 0) Executive Summary

RZN Integrations already exposes connectors as **tools** with **JSON Schemas** (MCP-compatible).
However, tool *outputs* are currently connector-specific, which forces downstream systems to
implement N different normalizers for N connectors.

This spec defines:

1) A **standard pagination input/output contract** (cursor/limit/has_more),
2) A **normalized output format** (`normalized_v1`) that tools can return (opt-in) for ingestion,
3) A stable, connector-agnostic set of types:
   - `ContentItem` (document-level),
   - `ContentBlock` (chunk-ready, anchorable),
   - `Relationship` (thread/reply graph),
   - `Truncation` (huge threads/channels without blowing up memory).

The normalized output is designed to feed the same downstream pipeline regardless of source:

`discover/list → fetch/get → normalize → store revision + blocks → chunk → BM25 + vectors → (optional) cards/facets`.

This does **not** change MCP; tools remain tools. This is simply an optional output format under `structured_content`.

---

## 1) Goals / Non-Goals

### Goals

- **Downstream stability**: downstream indexers can ingest Reddit/HN/YouTube/Discord/etc. without per-connector parsing code.
- **MCP interoperability**: remain compatible with MCP “tools” model; no new protocol.
- **Scheduled + on-demand**: support both “crawler mode” (scheduled listing) and “import this URL/id”.
- **Full commentary support**: threads can include all comments/messages while remaining bounded and resumable.
- **Explainable partial results**: first-class truncation/partial flags (limits, reasons, counts).

### Non-Goals (v1)

- Perfect “semantic normalization” across all domains (we normalize structure, not meaning).
- A universal schema for every connector’s metadata (we provide a place for metadata; connectors can extend).
- Guaranteeing legality/compliance of every connector. Downstream should gate connectors by distribution/feature flags.

---

## 2) Terminology

- **Connector**: an integration (e.g., `reddit`, `hackernews`, `youtube`, `discord`).
- **Tool**: a connector capability exposed via MCP (e.g., `reddit.list`, `reddit.get`).
- **List/Search tool**: returns multiple items with pagination (discover candidates).
- **Get tool**: fetches a single item in full detail for ingestion.
- **Item**: the document-level unit to ingest (thread, video, paper, channel window, email thread).
- **Block**: a sub-unit inside an item that becomes an indexable chunk (comment/message/transcript segment).
- **Cursor**: an opaque pagination token.
- **Normalized output**: a stable JSON structure for downstream ingestion, requested by clients via `output_format`.

---

## 3) Backwards Compatibility and Opt-in Behavior

This spec is **opt-in**.

- Tools continue returning their existing outputs by default.
- When a client passes `output_format="normalized_v1"`, the tool returns `structured_content` in the normalized shape.
- CLI commands may keep their current UX. The contract is at the tool layer.

Downstream indexers should:
- request `output_format="normalized_v1"` when they want ingestion-ready results,
- fall back to legacy outputs if a tool doesn’t support normalized output yet.

---

## 4) Standard Tool Inputs (Pagination + Output Format)

Any list/search tool **SHOULD** accept these optional fields:

- `limit?: integer`
  Connector clamps to a safe max.
- `cursor?: string | null`
  Opaque token from a prior call. `null` means “first page”.
- `output_format?: "raw" | "normalized_v1"`
  Defaults to `"raw"`.

Any get tool **SHOULD** accept:

- `output_format?: "raw" | "normalized_v1"` (default `"raw"`)

### Cursor rule (normative)

Connectors must treat `cursor` as opaque, but they should:
- make cursors **stable** for the same query parameters,
- allow cursors to be persisted by downstream systems.

Implementation guidance:
- Encode a small JSON struct (page/offset/after_id) as base64url.

---

## 5) Normalized Output Types (v1)

Normalized output is always placed in `structured_content`.

### 5.1 Top-level shapes

There are two valid normalized responses:

1) **Normalized Page** (`rzn-tools.normalized_page.v1`) — returned by list/search tools
2) **Normalized Item** (`rzn-tools.normalized_item.v1`) — returned by get tools

#### 5.1.1 `rzn-tools.normalized_page.v1`

```json
{
  "type": "rzn-tools.normalized_page.v1",
  "items": [ /* ContentItem[] */ ],
  "next_cursor": "opaque-or-null",
  "has_more": true,
  "partial": {
    "is_partial": false,
    "reason": null,
    "limits": { "max_items": 500 }
  },
  "source": {
    "connector": "reddit",
    "tool": "list",
    "fetched_at": "2025-12-28T22:10:00Z"
  }
}
```

Required fields:
- `type`, `items`, `has_more`, `partial`, `source`

#### 5.1.2 `rzn-tools.normalized_item.v1`

```json
{
  "type": "rzn-tools.normalized_item.v1",
  "item": { /* ContentItem */ },
  "partial": {
    "is_partial": true,
    "reason": "comment_limit",
    "limits": { "max_blocks_per_item": 300 }
  },
  "source": {
    "connector": "reddit",
    "tool": "get",
    "fetched_at": "2025-12-28T22:12:00Z"
  }
}
```

Required fields:
- `type`, `item`, `partial`, `source`

### 5.2 `ContentItem` (document-level)

`ContentItem` represents the unit to store as a downstream “Document” (thread/video/paper/email thread/etc.).

```json
{
  "item_ref": "reddit:post:1pxovbd",
  "kind": "thread",
  "canonical_url": "https://www.reddit.com/r/ScienceBasedParenting/comments/1pxovbd/...",
  "title": "When does yelling become abusive?",
  "created_at": "2025-12-27T12:34:56Z",
  "source_updated_at": null,

  "authors": [{ "name": "Jumpingapplecar", "id": null }],
  "tags": ["ScienceBasedParenting"],

  "metadata": {
    "subreddit": "ScienceBasedParenting",
    "score": 12,
    "num_comments": 17
  },

  "blocks": [ /* ContentBlock[] */ ],
  "relationships": [ /* Relationship[] */ ],
  "truncation": { /* Truncation */ }
}
```

Required fields (normative):
- `item_ref: string` — stable identifier
- `kind: string` — e.g. `thread`, `video`, `paper`, `channel_window`, `message_thread`
- `blocks: ContentBlock[]` — may be empty if no text

Recommended fields:
- `canonical_url`, `title`, `created_at`, `source_updated_at`, `authors`, `tags`, `metadata`

### 5.3 `ContentBlock` (chunk-ready)

Blocks are the “atomic text units” that downstream will usually chunk/index.

```json
{
  "block_ref": "reddit:comment:nwd096a",
  "block_kind": "comment",
  "text": "This is kind of a general write up that links to ...",
  "author": { "name": "jessicat62993", "id": null },
  "created_at": "2025-12-27T13:00:00Z",

  "reply_to": "reddit:comment:nwci1cz",
  "position": { "kind": "thread_depth", "depth": 0 },
  "score": 46,
  "attachments": [{ "kind": "link", "url": "https://www.nami.org/advocate/the-problem-with-yelling/" }],
  "metadata": { "permalink": "https://www.reddit.com/..." }
}
```

Required fields:
- `block_ref: string` — stable identifier
- `block_kind: string` — connector-defined but should be consistent (`comment`, `message`, `transcript_segment`, `post_body`, …)
- `text: string`

Optional fields:
- `author`, `created_at`, `reply_to`, `position`, `score`, `attachments`, `metadata`

### 5.4 `Relationship` (thread graph)

Relationships preserve structure without forcing downstream to parse nested trees.

```json
{ "rel": "replies_to", "from": "reddit:comment:nwdb1t9", "to": "reddit:comment:nwd096a" }
```

Required fields:
- `rel: string` — `replies_to`, `has_block`, `mentions`, `quotes`, `links_to`
- `from: string`, `to: string`

### 5.5 `Truncation` and partial results (huge threads/channels)

Truncation is **required** when an item is not fully included due to limits.

```json
{
  "is_truncated": true,
  "reason": "max_blocks_per_item",
  "total_blocks_hint": 5200,
  "returned_blocks": 300,
  "policy": "top_by_score_then_replies"
}
```

`partial` is for the overall tool call; `truncation` is per-item.

Normative behavior:
- If a connector enforces a cap (comments/messages/transcript segments), it must:
  - set `item.truncation.is_truncated = true`
  - include `returned_blocks`
  - include either `total_blocks_hint` or `reason` explaining why total is unknown.

---

## 6) Identifier Rules (item_ref and block_ref)

### 6.1 Requirements

- IDs must be stable across calls for the same underlying object.
- IDs must be unique within the connector namespace.

### 6.2 Recommended format

Use a colon-separated format:

- `item_ref = "{connector}:{item_kind}:{native_id}"`
- `block_ref = "{connector}:{block_kind}:{native_id}"`

Examples:
- `reddit:post:1pxovbd`
- `reddit:comment:nwd096a`
- `hackernews:story:46408988`
- `hackernews:comment:123456789` (when available)
- `youtube:video:dQw4w9WgXcQ`
- `youtube:segment:{video_id}:{start_ms}-{end_ms}`
- `discord:message:123456789012345678`

When the native ID is a URL, normalize it first (remove fragments, common tracking params, stable host casing).

---

## 7) Connector-Specific Mapping Notes (v1 targets)

This section is intentionally prescriptive for the first batch of connectors to implement.

### 7.1 Reddit

List/search tools:
- Normalize as `rzn-tools.normalized_page.v1` with `items[]` where each item may have empty `blocks[]` (discovery), or can include lightweight `blocks[]` (optional).

Recommended discovery recipes:

- “Top threads of all time” (best for bootstrapping a high-signal corpus):
  - `reddit/list` with `{ sort:"top", time:"all", limit:500, output_format:"normalized_v1" }`
- “Nightly refresh” (low-churn incremental ingestion):
  - `reddit/list` with `{ sort:"new", limit:100, cursor, output_format:"normalized_v1" }`
  - downstream persists `cursor` and replays a small backfill window (e.g., 3 days) if cursor is time-based.

Get tool (thread):
- `ContentItem.kind = "thread"`
- Add one block for post body:
  - `block_kind = "post_body"`
  - `block_ref = "reddit:post_body:{post_id}"` (or reuse post id)
- Add one block per comment:
  - `block_kind = "comment"`
  - `block_ref = "reddit:comment:{comment_id}"`
  - `reply_to` set when parent exists
- Add `relationships`:
  - `has_block` edges for all blocks
  - `replies_to` edges for comment reply structure

Truncation policy (recommended):
- If `num_comments > 5000`, default to top N comments by score (plus reply chains up to depth M), instead of trying to fetch/emit everything in one call.
- Set `truncation` with `policy="top_by_score_then_replies"`, and include `total_blocks_hint=num_comments` when available.

Default caps (suggested):
- `max_blocks_per_item = 300` when `num_comments > 5000`
- otherwise `max_blocks_per_item = 2000` (still bounded)

### 7.2 Hacker News

rzn-tools currently returns nested comments with `text` and `comments[]`. For ingestion:

- Flatten into blocks while preserving `replies_to` edges.
- Ensure block refs are stable. If the upstream API does not provide comment IDs in the used mode, the connector must:
  - switch to a response mode that includes IDs, OR
  - synthesize deterministic IDs (e.g., hash of path + text + index) as a fallback, and mark it in metadata.

### 7.3 YouTube

Get tool should return:
- `ContentItem.kind = "video"`
- `blocks`: transcript segments (prefer). If only full transcript text is available, emit segments by splitting with timestamps if possible.
- Each transcript segment should carry:
  - `position.kind = "time_range"`
  - `position.start_ms`, `position.end_ms`

### 7.4 Discord

Discord has multiple “conversation shapes”: channels, threads, DMs. v1 supports both:

Recommended documentization:
- **Thread**: `ContentItem.kind="thread"`, blocks = messages
- **Busy flat channel**: `ContentItem.kind="channel_window"`, blocks = a bounded window of messages (e.g., 200), typically newest-first for discovery

Recommended identifiers:
- `item_ref = "discord:thread:{thread_id}"` for a thread
- `item_ref = "discord:channel_window:{channel_id}:{start_message_id}-{end_message_id}"` for a window
- `block_ref = "discord:message:{message_id}"`

Required connector upgrades for archival indexing:
- `read_messages` needs pagination inputs (cursor/before/after), not just `limit`.
- If thread listing is supported, expose thread IDs explicitly (threads can be treated like channels for reads).

Truncation:
- When limiting messages in a channel window, set `truncation` with a policy like `newest_first_window`.

---

## 8) Suggested “Downstream Ingestion Recipe” (how to use this contract)

Downstream systems (desktop/backend) should implement a “SourceRecipe” per use case that binds tools + budgets:

- discovery:
  - tool: `reddit.list` (or `reddit.search`)
  - args: `{ subreddit, sort, time, limit, cursor, output_format:"normalized_v1" }`
- fetch:
  - tool: `reddit.get`
  - args: `{ post_url, comment_sort, comment_limit, output_format:"normalized_v1" }`
- budgets:
  - `max_items_per_run`
  - `max_blocks_per_item`
  - `max_total_chars_per_run`

The normalized output is already block/chunk shaped; downstream can:
- store `ContentItem` as a document + revision,
- store `ContentBlock`s as “chunks”,
- carry `block_ref` as the universal anchor.

---

## 8.1 Practical policy presets (recommended)

These are suggested “safe defaults” for downstream ingestion pipelines that want full commentary but bounded work.

### Social thread preset (Reddit/HN)

- `max_blocks_per_item_default = 2000`
- `max_blocks_per_item_huge_thread = 300` (when `total_blocks_hint > 5000`)
- `max_total_chars_per_item = 2_000_000` (to cap pathological cases)
- prefer “top by score” for truncation, and emit `truncation`

### Chat stream preset (Discord channels)

- `channel_window_size = 200` for discovery
- `channel_window_backfill = 500` for scheduled indexing (over time)
- use pagination to walk history gradually (cursor/before) instead of fetching everything at once

---

## 12) Worked Examples (normalized_v1)

These examples are illustrative; real connectors will include additional metadata.

### 12.1 Reddit list (top/new) → `rzn-tools.normalized_page.v1`

```json
{
  "type": "rzn-tools.normalized_page.v1",
  "items": [
    {
      "item_ref": "reddit:post:1pxovbd",
      "kind": "thread",
      "canonical_url": "https://www.reddit.com/r/ScienceBasedParenting/comments/1pxovbd/when_does_yelling_become_abusive/",
      "title": "When does yelling become abusive?",
      "created_at": "2025-12-27T00:00:00Z",
      "authors": [{ "name": "Jumpingapplecar", "id": null }],
      "tags": ["ScienceBasedParenting"],
      "metadata": { "score": 11, "num_comments": 17 },
      "blocks": []
    }
  ],
  "next_cursor": null,
  "has_more": false,
  "partial": { "is_partial": false, "reason": null, "limits": { "max_items": 500 } },
  "source": { "connector": "reddit", "tool": "list", "fetched_at": "2025-12-28T22:10:00Z" }
}
```

### 12.2 Reddit get (thread + comments) → `rzn-tools.normalized_item.v1`

```json
{
  "type": "rzn-tools.normalized_item.v1",
  "item": {
    "item_ref": "reddit:post:1pxovbd",
    "kind": "thread",
    "canonical_url": "https://www.reddit.com/r/ScienceBasedParenting/comments/1pxovbd/when_does_yelling_become_abusive/",
    "title": "When does yelling become abusive?",
    "authors": [{ "name": "Jumpingapplecar", "id": null }],
    "tags": ["ScienceBasedParenting"],
    "metadata": { "score": 12, "num_comments": 17 },
    "blocks": [
      {
        "block_ref": "reddit:post_body:1pxovbd",
        "block_kind": "post_body",
        "text": "Hello everybody, my child is still a baby...",
        "author": { "name": "Jumpingapplecar", "id": null }
      },
      {
        "block_ref": "reddit:comment:nwd096a",
        "block_kind": "comment",
        "text": "This is kind of a general write up that links to more scientific studies: ...",
        "author": { "name": "jessicat62993", "id": null },
        "score": 46,
        "reply_to": null
      },
      {
        "block_ref": "reddit:comment:nwdb1t9",
        "block_kind": "comment",
        "text": "Piggybacking here because I don't have an article but ...",
        "author": { "name": "thisismypregnantname", "id": null },
        "score": 26,
        "reply_to": "reddit:comment:nwd096a"
      }
    ],
    "relationships": [
      { "rel": "has_block", "from": "reddit:post:1pxovbd", "to": "reddit:post_body:1pxovbd" },
      { "rel": "has_block", "from": "reddit:post:1pxovbd", "to": "reddit:comment:nwd096a" },
      { "rel": "has_block", "from": "reddit:post:1pxovbd", "to": "reddit:comment:nwdb1t9" },
      { "rel": "replies_to", "from": "reddit:comment:nwdb1t9", "to": "reddit:comment:nwd096a" }
    ],
    "truncation": null
  },
  "partial": { "is_partial": false, "reason": null, "limits": { "max_blocks_per_item": 2000 } },
  "source": { "connector": "reddit", "tool": "get", "fetched_at": "2025-12-28T22:12:00Z" }
}
```

### 12.3 Hacker News story (flattened comments)

```json
{
  "type": "rzn-tools.normalized_item.v1",
  "item": {
    "item_ref": "hackernews:story:46408988",
    "kind": "thread",
    "canonical_url": "https://news.ycombinator.com/item?id=46408988",
    "title": "Growing up in “404 Not Found”…",
    "blocks": [
      {
        "block_ref": "hackernews:story_body:46408988",
        "block_kind": "post_body",
        "text": ""
      },
      {
        "block_ref": "hackernews:comment:0001",
        "block_kind": "comment",
        "text": "Hi HN, OP here. I grew up in \"Factory 404\" ...",
        "reply_to": null
      },
      {
        "block_ref": "hackernews:comment:0002",
        "block_kind": "comment",
        "text": "Thanks Vincent for submitting, this is really fascinating.",
        "reply_to": "hackernews:comment:0001"
      }
    ],
    "relationships": [
      { "rel": "replies_to", "from": "hackernews:comment:0002", "to": "hackernews:comment:0001" }
    ]
  },
  "partial": { "is_partial": true, "reason": "example_truncated", "limits": null },
  "source": { "connector": "hackernews", "tool": "get_post", "fetched_at": "2025-12-28T22:15:00Z" }
}
```

**Note**: Replace `hackernews:comment:0001` synthetic IDs with real HN comment IDs where possible.

### 12.4 Discord channel window (newest-first) → `channel_window`

```json
{
  "type": "rzn-tools.normalized_item.v1",
  "item": {
    "item_ref": "discord:channel_window:456:900-1100",
    "kind": "channel_window",
    "canonical_url": null,
    "title": "#flux",
    "metadata": { "guild_id": "123", "channel_id": "456" },
    "blocks": [
      {
        "block_ref": "discord:message:1099",
        "block_kind": "message",
        "text": "Flux tip: try cfg=3.5 with ...",
        "author": { "name": "alice", "id": "discord:user:42" },
        "created_at": "2025-12-28T21:00:00Z"
      }
    ],
    "truncation": {
      "is_truncated": true,
      "reason": "window_limit",
      "total_blocks_hint": null,
      "returned_blocks": 200,
      "policy": "newest_first_window"
    }
  },
  "partial": { "is_partial": false, "reason": null, "limits": { "window_size": 200 } },
  "source": { "connector": "discord", "tool": "read_messages", "fetched_at": "2025-12-28T22:18:00Z" }
}
```

---

## 9) JSON Schema (reference; optional to ship as separate files)

This section is a reference schema sketch (draft-07 compatible). Implementers may choose to:
- embed these schemas in code for validation, and/or
- ship them under `docs/` as `.json` files.

**NOTE**: This is intentionally conservative to avoid blocking connector authors.

```json
{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "$id": "https://rzn-tools.ai/schemas/ingest/normalized_v1.json",
  "title": "rzn-tools Ingest Normalized Output v1",
  "oneOf": [
    { "$ref": "#/definitions/NormalizedPageV1" },
    { "$ref": "#/definitions/NormalizedItemV1" }
  ],
  "definitions": {
    "Partial": {
      "type": "object",
      "required": ["is_partial"],
      "properties": {
        "is_partial": { "type": "boolean" },
        "reason": { "type": ["string", "null"] },
        "limits": { "type": ["object", "null"], "additionalProperties": true }
      },
      "additionalProperties": false
    },
    "Source": {
      "type": "object",
      "required": ["connector", "tool", "fetched_at"],
      "properties": {
        "connector": { "type": "string" },
        "tool": { "type": "string" },
        "fetched_at": { "type": "string" }
      },
      "additionalProperties": true
    },
    "Author": {
      "type": "object",
      "required": ["name"],
      "properties": {
        "name": { "type": "string" },
        "id": { "type": ["string", "null"] }
      },
      "additionalProperties": true
    },
    "Attachment": {
      "type": "object",
      "required": ["kind"],
      "properties": {
        "kind": { "type": "string" },
        "url": { "type": ["string", "null"] },
        "title": { "type": ["string", "null"] }
      },
      "additionalProperties": true
    },
    "Truncation": {
      "type": "object",
      "required": ["is_truncated", "reason", "returned_blocks"],
      "properties": {
        "is_truncated": { "type": "boolean" },
        "reason": { "type": "string" },
        "total_blocks_hint": { "type": ["integer", "null"] },
        "returned_blocks": { "type": "integer" },
        "policy": { "type": ["string", "null"] }
      },
      "additionalProperties": true
    },
    "Relationship": {
      "type": "object",
      "required": ["rel", "from", "to"],
      "properties": {
        "rel": { "type": "string" },
        "from": { "type": "string" },
        "to": { "type": "string" }
      },
      "additionalProperties": true
    },
    "ContentBlock": {
      "type": "object",
      "required": ["block_ref", "block_kind", "text"],
      "properties": {
        "block_ref": { "type": "string" },
        "block_kind": { "type": "string" },
        "text": { "type": "string" },
        "author": { "$ref": "#/definitions/Author" },
        "created_at": { "type": ["string", "null"] },
        "reply_to": { "type": ["string", "null"] },
        "position": { "type": ["object", "null"], "additionalProperties": true },
        "score": { "type": ["number", "null"] },
        "attachments": { "type": "array", "items": { "$ref": "#/definitions/Attachment" } },
        "metadata": { "type": ["object", "null"], "additionalProperties": true }
      },
      "additionalProperties": true
    },
    "ContentItem": {
      "type": "object",
      "required": ["item_ref", "kind", "blocks"],
      "properties": {
        "item_ref": { "type": "string" },
        "kind": { "type": "string" },
        "canonical_url": { "type": ["string", "null"] },
        "title": { "type": ["string", "null"] },
        "created_at": { "type": ["string", "null"] },
        "source_updated_at": { "type": ["string", "null"] },
        "authors": { "type": "array", "items": { "$ref": "#/definitions/Author" } },
        "tags": { "type": "array", "items": { "type": "string" } },
        "metadata": { "type": ["object", "null"], "additionalProperties": true },
        "blocks": { "type": "array", "items": { "$ref": "#/definitions/ContentBlock" } },
        "relationships": { "type": "array", "items": { "$ref": "#/definitions/Relationship" } },
        "truncation": { "anyOf": [{ "$ref": "#/definitions/Truncation" }, { "type": "null" }] }
      },
      "additionalProperties": true
    },
    "NormalizedPageV1": {
      "type": "object",
      "required": ["type", "items", "has_more", "partial", "source"],
      "properties": {
        "type": { "const": "rzn-tools.normalized_page.v1" },
        "items": { "type": "array", "items": { "$ref": "#/definitions/ContentItem" } },
        "next_cursor": { "type": ["string", "null"] },
        "has_more": { "type": "boolean" },
        "partial": { "$ref": "#/definitions/Partial" },
        "source": { "$ref": "#/definitions/Source" }
      },
      "additionalProperties": false
    },
    "NormalizedItemV1": {
      "type": "object",
      "required": ["type", "item", "partial", "source"],
      "properties": {
        "type": { "const": "rzn-tools.normalized_item.v1" },
        "item": { "$ref": "#/definitions/ContentItem" },
        "partial": { "$ref": "#/definitions/Partial" },
        "source": { "$ref": "#/definitions/Source" }
      },
      "additionalProperties": false
    }
  }
}
```

---

## 10) Rollout Plan (recommended)

1) **Core**: add shared normalized types in `rzn_tools_core` and a `NormalizedOutputV1` helper.
2) **First connectors**: implement `output_format="normalized_v1"` for:
   - Reddit: list/search/get
   - Hacker News: top/search/story
   - YouTube: get/list/search (at least get)
   - Discord: read_messages (+ pagination)
3) **Docs**: update `docs/CONNECTOR_DEVELOPMENT.md` with:
   - how to add normalized output,
   - required identifier stability,
   - truncation semantics.
4) **Downstream adoption**: downstream indexers switch from parsing raw outputs to requesting normalized output.

---

## 11) Testing Guidance

Minimum tests:
- Unit test: normalized structures serialize deterministically.
- Unit test: cursor encoding/decoding round-trips (if cursor is structured).
- Connector tests: normalized output for a known fixture (mocked API responses).
- Regression test: existing raw outputs remain unchanged when `output_format` is omitted.

Avoid real network calls in CI; use fixtures/mocks.
