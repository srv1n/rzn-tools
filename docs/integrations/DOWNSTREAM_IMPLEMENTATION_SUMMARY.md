# Downstream Implementation Summary (CLI Ingestion)

**Status**: Draft (implementation-facing)
**Last updated**: 2025-12-29
**Scope**: Downstream team deliverables (generic ingestion config + scheduler + indexing)

This document summarizes the downstream work implemented in `rzn_tools_cli` to enable a
zero-hardcoding ingestion loop using `connectors/ingest_sources` and normalized outputs.

---

## 1) What Was Implemented

### A) Ingest discovery

New CLI surface:

- `rzn-tools ingest sources`
  - Calls `connectors/ingest_sources` (MCP server helper).
  - Supports filters: `--connectors`, `--categories`, `--include-read`, `--include-fetch`.
  - Pretty table output or JSON/YAML via `--output`.

### B) Ingest configuration (per tenant)

New CLI surface:

- `rzn-tools ingest add <id> --args '{...}' [--tenant X]`
- `rzn-tools ingest list [--tenant X]`
- `rzn-tools ingest remove <id> [--tenant X]`

Config storage:

- Default tenant: `~/.config/rzn-tools/ingest_sources.json`
- Named tenant: `~/.config/rzn-tools/tenants/<tenant>/ingest_sources.json`

Each configured source stores:

- `id`, `connector`, `display_name`, `tool`, `category`, `tags`
- `default_args` (from ingest catalog)
- `args` (tenant/source-specific overrides)
- `enabled`, `cadence_seconds`
- `last_cursor`, `last_run_at`, `last_error`

### C) Ingestion scheduler loop

New CLI surface:

- `rzn-tools ingest run [--tenant X] [--id <source>]`
  - `--max-pages <N>` (default: 1)
  - `--max-items <N>` (per source)
  - `--interval-seconds <N>` (repeat loop)
  - `--include-disabled` (optional)

Behavior:

1) Merges `default_args + args`.
2) Injects `output_format = "normalized_v1"` (always).
3) Injects `cursor` from stored state if present.
4) Calls `tools/call` via `McpServer::handle_call_tool(...)`.
5) Expects normalized outputs:
   - `type="rzn-tools.normalized_page.v1"` for pageable tools
   - `type="rzn-tools.normalized_item.v1"` for fetch tools
6) Stores `next_cursor` (if present) and timestamps/errors.

### D) Indexing pipeline (local JSONL)

Per tenant output directory:

- Default tenant: `~/.config/rzn-tools/ingest/`
- Named tenant: `~/.config/rzn-tools/tenants/<tenant>/ingest/`

Files:

- `items.jsonl` (one record per `ContentItem`)
- `blocks.jsonl` (one record per `ContentBlock`)
- `seen_items.txt` / `seen_blocks.txt` (de-dupe keys)

De-dupe behavior:

- Items deduped by `item_ref`
- Blocks deduped by `block_ref`

Record shape (JSONL):

```json
{
  "item": { "...": "ContentItem" },
  "source": { "connector": "...", "tool": "...", "fetched_at": "..." },
  "partial": { "is_partial": false }
}
```

```json
{
  "item_ref": "x:item:1",
  "block": { "...": "ContentBlock" },
  "source": { "connector": "...", "tool": "...", "fetched_at": "..." }
}
```

---

## 2) Code Locations

- CLI command + ingestion implementation:
  - `rzn_tools_cli/src/commands/ingest.rs`
  - `rzn_tools_cli/src/cli.rs` (new `ingest` subcommand)
  - `rzn_tools_cli/src/main.rs` (dispatch)
  - `rzn_tools_cli/src/commands/mod.rs` (module export)

---

## 3) Example Usage

```bash
# Discover ingest sources
rzn-tools ingest sources

# Add a source (example: reddit list)
rzn-tools ingest add reddit:list --args '{"subreddit":"rust","sort":"top","time":"week"}'

# List configured sources
rzn-tools ingest list

# Run ingestion (single page)
rzn-tools ingest run --max-pages 1
```

---

## 4) Known Limitations / Follow-ups

- Per-source `cadence_seconds` is stored but not enforced yet (scheduler uses CLI `--interval-seconds`).
- No automatic “fetch full item” hint consumption; list/search results are indexed as-is.
- De-dupe is `item_ref` / `block_ref` only (no content hashing).
- No backoff/retry policy beyond re-running on the next loop.

---

## 5) Review Targets (Upstream)

- Ensure ingestion calls are **normalized-only** (`output_format="normalized_v1"`).
- Confirm config + index paths are acceptable for downstream hosts.
- Validate JSONL shapes match expected ingestion payloads.
- Validate cursor persistence and `has_more` behavior.
