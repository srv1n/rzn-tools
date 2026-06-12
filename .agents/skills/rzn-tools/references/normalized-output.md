# Normalized Output Reference

Use this when adding ingestion/indexing support or reviewing `output_format=normalized_v1`.

## Contents

- [Contract](#contract)
- [Standard Inputs](#standard-inputs)
- [Output Shapes](#output-shapes)
- [Required Invariants](#required-invariants)
- [Schema Advertising](#schema-advertising)
- [Implementation Helpers](#implementation-helpers)

## Contract

`normalized_v1` is opt-in. Existing tools keep returning raw connector-specific output by default.

Indexable tools are:

- list/feed tools,
- search tools,
- get/read-one tools,
- windowed history reads such as messages/comments.

Mutation/admin tools usually stay raw.

## Standard Inputs

Indexable tools should accept:

```json
{
  "limit": 100,
  "cursor": null,
  "output_format": "normalized_v1"
}
```

Search-like tools may also accept:

- `locale`
- `language`
- `region`
- `since`
- `until`
- `date_preset`
- `include_domains`
- `exclude_domains`

Canonical get tools should accept at least one of:

- `item_ref`
- `url`

Legacy identifiers may remain optional, such as `video_id`, `paper_id`, or `pmid`.

## Output Shapes

List/search/pageable tools return a normalized page:

```json
{
  "type": "rzn-tools.normalized_page.v1",
  "items": [],
  "next_cursor": null,
  "has_more": false,
  "partial": {
    "is_partial": false,
    "reason": null,
    "limits": {}
  },
  "source": {
    "connector": "reddit",
    "tool": "list",
    "fetched_at": "2026-04-28T00:00:00Z"
  }
}
```

Get/read-one tools return a normalized item:

```json
{
  "type": "rzn-tools.normalized_item.v1",
  "item": {
    "item_ref": "hackernews:story:8863",
    "kind": "thread",
    "blocks": []
  },
  "partial": {
    "is_partial": false,
    "reason": null,
    "limits": {}
  },
  "source": {
    "connector": "hackernews",
    "tool": "get",
    "fetched_at": "2026-04-28T00:00:00Z"
  }
}
```

## Required Invariants

- Put normalized payloads in `CallToolResult.structured_content`.
- Prefer empty `content` for normalized output to avoid duplicate payloads.
- `next_cursor` belongs at the top level of the page.
- `has_more` is true iff `next_cursor` is present.
- Cursors are opaque and safe to persist.
- Invalid cursors return `InvalidParams`.
- `item_ref` format should be `{connector}:{kind}:{native_id}`.
- Use base64url or another safe encoding when native IDs can contain separators.

## Schema Advertising

If a tool supports normalized output, its input schema should include:

```json
{
  "output_format": {
    "type": "string",
    "enum": ["raw", "normalized_v1"],
    "default": "raw",
    "description": "Default raw. Use normalized_v1 for ingestion pipelines."
  }
}
```

If it supports paging:

```json
{
  "cursor": {
    "type": ["string", "null"],
    "description": "Opaque cursor from a previous response."
  },
  "limit": {
    "type": "integer",
    "description": "Max results (connector clamps to safe max)."
  }
}
```

Some existing connectors also advertise `display_v1`. Preserve it if the connector already supports it.

## Implementation Helpers

Look for these helpers in `rzn_tools_core` before writing new parsing code:

- `ingest::output_format_from_args(...)`
- `ingest::encode_cursor(...)`
- `ingest::decode_cursor(...)`
- `ingest::parse_item_ref_for_connector(...)`
- `utils::structured_result(...)`
- `utils::structured_result_with_text(...)`

Use existing implementations as examples:

- `rzn_tools_core/src/connectors/hackernews/mod.rs`
- `rzn_tools_core/src/connectors/rss/mod.rs`
- `rzn_tools_core/src/connectors/web/mod.rs`
