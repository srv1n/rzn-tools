# RZN Integrations Normalization Spec v1 (Standard Inputs + Normalized Outputs)

**Status**: Draft (implementation-facing)
**Last updated**: 2025-12-29
**Audience**: Connector authors, downstream hosts (desktop/server), ingestion/indexing pipelines
**Primary goal**: add connectors without downstream custom parsing, pagination, or ID-mapping code.

This is the *implementation* spec: what connector authors must implement and what downstream
hosts can rely on.

Related documents:
- Background + examples: `docs/integrations/INGEST_CONTRACT_V1.md`
- Launcher discovery: `docs/integrations/LAUNCHER_INTEGRATION_SPEC.md`
- Downstream tool surface conventions: `docs/integrations/DOWNSTREAM_UPGRADE.md`

---

## 0) Goals / Non-Goals

### Goals

1) **Uniform discovery**:
   - connectors and tools must “broadcast” what they support through MCP (`connectors/list`,
     `tools/list`) so hosts do not hardcode.
2) **Uniform invocation**:
   - indexable tools accept a shared base input contract (`limit`, opaque `cursor`,
     `output_format`) so ingestion loops are generic.
3) **Uniform indexing output**:
   - tools can return connector-specific `"raw"` outputs, but **also** an opt-in normalized output
     (`"normalized_v1"`) that downstream can ingest without custom parsing.
4) **Safe, bounded outputs**:
   - huge threads/channels must be truncated in a principled, explainable way.

### Non-Goals (v1)

- Perfect semantic normalization across domains (we normalize structure + IDs, not meaning).
- Forcing mutation/admin tools (send email, post message, create ticket) into the normalized
  document model.
- Making compliance decisions; hosts/tenants must decide what connectors are enabled.

---

## 1) Normative Language

The keywords **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** are interpreted as in
RFC 2119.

---

## 2) Scope: Which Tools Must Implement This

This spec applies to *indexable* tools:

- **List** tools (`connector/list`): discovery over a feed or collection.
- **Search** tools (`connector/search`): keyword discovery.
- **Get/Read** tools (`connector/get`): fetch a single item in full detail.
- **Windowed reads** that page history (e.g. chat/message windows): treat as list-like.

This spec does **not** require mutation tools to return normalized documents. They may continue
returning raw outputs only.

---

## 3) Standard Tool Inputs (Normative)

### 3.1 `output_format`

All indexable tools **MUST** accept:

- `output_format?: "raw" | "normalized_v1"` (default: `"raw"`)

Behavior:
- `"raw"` returns existing connector-specific structured output (plus any legacy text fallbacks).
- `"normalized_v1"` returns normalized output in `CallToolResult.structured_content`.

Implementation guidance:
- Parse via `rzn_tools_core::ingest::output_format_from_args(...)` or (for typed inputs) include an
  `output_format: OutputFormat` field with `#[serde(default)]`.

### 3.2 `limit` and `cursor` (pageable tools)

All pageable indexable tools **MUST** accept:

- `limit?: integer` (default per tool; connector clamps to safe max)
- `cursor?: string | null` (opaque token from a previous normalized response)

Rules:
- Missing/`null` cursor means “first page”.
- Cursor is tool-specific and opaque. Downstream callers must store and replay it verbatim.
- If cursor is invalid or mismatched, connector **MUST** return `InvalidParams`.

### 3.3 Shared search filters (optional but recommended)

For “search-like” tools, connectors **SHOULD** accept:

- `locale?: string` (e.g. `en-US`)
- `language?: string` (e.g. `en`)
- `region?: string` (e.g. `US`)
- `since?: string` (YYYY-MM-DD)
- `until?: string` (YYYY-MM-DD)
- `date_preset?: string` (`last_24_hours|last_7_days|last_30_days|this_month|last_365_days`)
- `include_domains?: string[]`
- `exclude_domains?: string[]`

Implementation guidance:
- Use `rzn_tools_core::utils::resolve_search_filters(...)` where applicable.

### 3.4 Schema requirement (how tools advertise the contract)

If a tool supports `output_format="normalized_v1"`, its `input_schema` **MUST** include an
`output_format` property with:

```json
{
  "type": "string",
  "enum": ["raw", "normalized_v1"],
  "default": "raw",
  "description": "Default raw. Use normalized_v1 for ingestion pipelines."
}
```

If the tool supports paging with `cursor`, its `input_schema` **MUST** include:

```json
{
  "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous response." },
  "limit": { "type": "integer", "description": "Max results (connector clamps to safe max)." }
}
```

### 3.5 Standard fetch inputs (canonical `connector/get`)

To allow zero-hardcoding “fetch full item” steps, canonical `connector/get` tools **MUST** accept
at least one of:

- `item_ref?: string` (preferred)
- `url?: string` (preferred when a canonical URL exists)

Legacy identifiers (e.g., `video_id`, `paper_id`, `pmid`) **MAY** remain, but **MUST** be optional
once `item_ref`/`url` is supported.

**`item_ref` format (recommended)**

```
{connector}:{kind}:{native_id}
```

Notes:
- The `native_id` segment **MAY** be encoded (e.g., base64url) when it can contain `:` or other
  unsafe separators.
- Connectors should parse with a `splitn(3)` style parser (see `parse_item_ref_for_connector` in
  `rzn_tools_core::ingest`).

The `url` input should accept the same canonical URLs emitted in `ContentItem.canonical_url`.

---

## 4) Cursor Semantics (Normative)

### 4.1 Opaque encoding

Connectors should encode a small JSON struct as base64url:
- `encode_cursor<T: Serialize>(...) -> String`
- `decode_cursor<T: DeserializeOwned>(...) -> Option<T>`

Requirements:
- Cursor **MUST** round-trip cleanly.
- Cursor **MUST** be safe to persist (e.g., in a DB) and reuse later.

### 4.2 Pagination placement invariants (important)

To keep downstream ingestion generic:

- Any tool that accepts a `cursor` **MUST** return a **Normalized Page**
  (`type="rzn-tools.normalized_page.v1"`).
- `next_cursor` **MUST** be returned at the **top level** of the page object.
- `has_more` **MUST** be `true` iff `next_cursor` is present.
- Tools **MUST NOT** place pagination cursors only inside `item.metadata` as the primary
  pagination path.

Rationale:
- Downstream ingestion loops implement exactly one pagination strategy.

---

## 5) Normalized Output Contract (Normative)

### 5.1 Where normalized output lives

When `output_format="normalized_v1"`, the connector **MUST** return normalized output in:

- `CallToolResult.structured_content`

And **SHOULD** set:
- `CallToolResult.content = []` (avoid mixed/duplicated payloads)

Implementation guidance:
- Use `rzn_tools_core::utils::structured_result(...)` for normalized payloads.

### 5.2 Allowed top-level shapes

There are exactly two valid normalized shapes:

1) **Page**: `type="rzn-tools.normalized_page.v1"` (list/search/pageable tools)
2) **Item**: `type="rzn-tools.normalized_item.v1"` (get/read-one tools)

These correspond to `NormalizedPageV1` / `NormalizedItemV1` in `rzn_tools_core/src/ingest.rs`.

---

## 6) Required Fields and Semantics

### 6.1 `ContentItem` (document-level)

Each normalized `ContentItem` **MUST** include:
- `item_ref: string` (stable identifier)
- `kind: string` (document class; examples: `thread`, `video`, `paper`, `file`, `channel_window`)
- `blocks: ContentBlock[]` (may be empty for discovery/listing pages)

Recommended fields (SHOULD when available):
- `canonical_url`, `title`, `created_at`, `source_updated_at`, `authors`, `tags`, `metadata`

### 6.2 `ContentBlock` (chunk-ready)

Each block **MUST** include:
- `block_ref: string` (stable identifier)
- `block_kind: string` (connector-defined but consistent; examples: `comment`, `message`,
  `post_body`, `transcript_segment`)
- `text: string`

Blocks **SHOULD** include `author` and `created_at` when available.

### 6.3 Identifiers (`item_ref` / `block_ref`)

Requirements:
- IDs **MUST** be stable across calls for the same underlying object.
- IDs **MUST** be unique within the connector namespace.

Recommended format:
- `item_ref = "{connector}:{item_kind}:{native_id}"`
- `block_ref = "{connector}:{block_kind}:{native_id}"`

If a connector cannot obtain a stable upstream ID:
- It **MAY** synthesize a deterministic ID (e.g., stable hash of `(path + timestamp + index)`),
  but it **MUST** be deterministic and **SHOULD** record `"id_synthesized": true` in item/block
  metadata.

### 6.4 Partial and truncation (bounded outputs)

Two distinct concepts exist:

- `partial` (top-level): describes the **tool call** being partial due to limits.
- `item.truncation` (per item): describes an **item** being truncated (huge thread, message window
  cap, etc.).

Normative behavior:
- If content is truncated due to connector-enforced caps, connector **MUST** set
  `item.truncation.is_truncated=true` and include `returned_blocks`.
- Connector **SHOULD** set `partial.is_partial=true` with a reason and include limits in
  `partial.limits`.

Recommended reason vocabulary:
- `max_items`, `max_blocks_per_item`, `window_limit`, `comment_limit`, `rate_limit`,
  `source_limit`, `unknown`

---

## 7) Tool Schema `_meta` + `examples` (Normative for indexable tools)

For indexable tools (list/search/get) that support normalized output, connectors **MUST** include:

- `examples` (non-empty array of example inputs)
- `_meta` object with:
  - `category`: one of `search|list|read|resolve|download|export|other`
  - `tags`: string[] (connector-specific)
  - `auth_required`: boolean
  - `supports_output_format`: boolean (true when `output_format` is supported)
  - `supports_cursor`: boolean (true when `cursor` is supported)

Notes:
- `_meta` is intentionally small but enables downstream UIs and schedulers to behave correctly
  without hardcoding connector names.

---

## 8) Downstream Host Integration (How “broadcasting” works)

Downstream hosts (desktop/server) should integrate as follows:

1) Compile desired connectors as Cargo features.
2) Register connectors automatically:
   - Use `build_registry_enabled_only()` to build a registry from compiled features.
3) Expose discovery + tools:
   - `connectors/list` and `tools/list` become the *only* discovery source for UIs and ingestion.
4) Runtime gating:
   - Hosts implement tenant/user allowlists on top of the compiled set.
   - Filter at the connector/tool layer; do not hardcode tool names in the host.

---

## 9) Conformance Tests (Required before “done”)

For every connector/tool marked as supporting `normalized_v1`, add tests that validate:

1) Schema conformance
   - `tools/list` includes `output_format` enum `["raw","normalized_v1"]`.
   - Pageable tools include `cursor` (string|null) and `limit` (integer).
   - `_meta.supports_output_format=true` and `_meta.supports_cursor` is correct.

2) Output conformance
   - Normalized outputs include `type` and required fields.
   - Pageable tools return `rzn-tools.normalized_page.v1` and set `next_cursor/has_more` consistently.

Guidance:
- Prefer fixture/snapshot tests. Avoid real network calls in CI.
- If mocking is easier, add a tiny dummy connector inside `rzn_tools_core/tests/...`.

---

## 10) Compatibility and Versioning

- `output_format` defaults to `"raw"`.
- `normalized_v1` is opt-in and must remain stable once released.
- Any breaking change must be introduced as a new `output_format` value (e.g. `"normalized_v2"`),
  and v1 must remain supported for a deprecation window.
