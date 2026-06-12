# Ingest Sources Endpoint Spec v1 (`connectors/ingest_sources`)

**Status**: Draft (implementation-facing)
**Last updated**: 2025-12-29
**Audience**: Downstream hosts (desktop/server), ingestion schedulers, connector authors
**Goal**: let hosts discover “what can be ingested” without hardcoding connector/tool names.

This endpoint returns a catalog of *ingestion sources* derived from each connector’s `tools/list`
surface, using tool schema `_meta` fields. Downstream systems should treat this as the default
entrypoint for “scheduled ingestion configuration”.

Related specs:
- Normalized output + paging contract: `docs/integrations/NORMALIZATION_SPEC_V1.md`
- Coverage work plan: `docs/integrations/CONNECTOR_COVERAGE_ROLLOUT.md`

---

## 1) Method

- **JSON-RPC method**: `connectors/ingest_sources`
- **Params**: optional object (see below)
- **Result**: `{ ingest_sources: IngestSource[] }`

---

## 2) Request Params

All params are optional.

```json
{
  "connectors": ["reddit", "hackernews"],
  "categories": ["list", "search", "read"],
  "include_read": true,
  "include_fetch": false
}
```

### `connectors?: string[]`

If provided (non-empty), only include sources from these connector names.

### `categories?: string[]`

If provided (non-empty), only include tools whose schema `_meta.category` matches one of these
values (case-insensitive).

Allowed category vocabulary (v1):
- `list`
- `search`
- `read`

**Note**: `resolve`, `download`, and `export` are always excluded from the ingest catalog (even if
explicitly requested), because they are not ingestion sources.

### `include_read?: boolean` (default: `true`)

Include “windowed read” tools:
- `_meta.category = "read"` AND
- `_meta.supports_cursor = true`

These tools are suitable for scheduled ingestion (e.g. chat/history windows).

### `include_fetch?: boolean` (default: `false`)

Include one-shot fetch tools:
- `_meta.category = "read"` AND
- `_meta.supports_cursor = false`

These are excluded by default because they are usually **not** good “sources” for scheduling (they
typically require a specific ID/URL and are better used as a *fetch step* after discovery).

---

## 3) Inclusion Rules (Normative)

A tool is included as an ingest source iff all of the following are true:

1) Tool schema contains `_meta.supports_output_format = true`
   (tools must support `output_format="normalized_v1"` for ingestion).
2) Tool schema contains `_meta.category` (string).
3) `_meta.category` is not one of: `resolve|download|export`.
4) Category eligibility:
   - `list` and `search` are always eligible
   - `read` is eligible only when:
     - `_meta.supports_cursor=true` and `include_read=true`, OR
     - `_meta.supports_cursor=false` and `include_fetch=true`

Rationale:
- Keeps ingestion scheduling generic (seed + page), without accidentally listing “fetch a single
  item” as a scheduled source.
- Downstream hosts can still call any tool via `tools/list` + `tools/call`; this catalog is only
  the default “what should be scheduled” list.

---

## 4) Response Shape

Each entry is an `IngestSource`:

```json
{
  "id": "reddit:list",
  "connector": "reddit",
  "display_name": "Reddit list",
  "description": "List posts in a subreddit",
  "tool": "reddit/list",
  "input_schema": { "...": "the tool input schema (including _meta + examples)" },
  "default_args": { "output_format": "normalized_v1", "limit": 25 },
  "category": "list",
  "tags": ["social", "community"],
  "auth_required": false
}
```

Field notes:
- `id`: stable identifier (format: `{connector}:{tool_name}` using the connector-local tool name)
- `tool`: callable tool name, **prefixed** as `{connector}/{tool_name}`
- `input_schema`: the tool schema (same shape as `tools/list`)
- `default_args`: baseline args that make ingestion safe + standardized

---

## 5) `default_args` Rules

`default_args` is intended to keep downstream ingestion loops **zero-custom-code**.

### Always set

- `output_format = "normalized_v1"`

### `limit` behavior

If the tool schema defines a `limit` property:

1) If `properties.limit.default` exists, use it.
2) Otherwise, set a suggested default:
   - `25` for `list`/`search`
   - `50` for `read`
3) If `properties.limit.minimum` / `maximum` exist, clamp the suggested default into that range.

### Cursor behavior

The ingest catalog does not set `cursor` in `default_args`.

Downstream ingestion should:
- omit `cursor` or pass `null` for the first call
- persist `next_cursor` from `rzn-tools.normalized_page.v1` and replay it verbatim for subsequent runs

---

## 6) Downstream Host Usage (Recommended)

### UI configuration flow

1) Call `connectors/list` to show available connectors and auth status.
2) Call `connectors/ingest_sources` to list the scheduler-ready sources.
3) Admin (or tenant) picks sources and provides tool-specific configuration by editing args that
   satisfy `input_schema` (e.g. subreddit, query, channel_id).

### Scheduler loop (generic)

For each configured source:

1) Build args:
   - start from `default_args`
   - merge in tenant/source-specific args
   - inject the stored `cursor` (or omit/null for first page)
2) Call `tools/call` on `source.tool`.
3) Expect `structured_content.type = "rzn-tools.normalized_page.v1"` for pageable tools.
4) Store `next_cursor` for the next run.
5) Index `items[].blocks[]` per `NORMALIZATION_SPEC_V1.md`.

---

## 7) Examples

### Default call (seed + windowed reads)

```json
{}
```

### Only list tools for a subset of connectors

```json
{ "connectors": ["reddit", "hackernews"], "categories": ["list"] }
```

### Read/window tools only (exclude one-shot fetches)

```json
{ "categories": ["read"], "include_fetch": false }
```

### Include one-shot fetch tools explicitly

```json
{ "categories": ["read"], "include_fetch": true }
```

