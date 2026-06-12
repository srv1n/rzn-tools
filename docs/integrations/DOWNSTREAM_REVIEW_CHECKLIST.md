# Downstream Review Checklist (CLI Ingestion)

**Status**: Draft (review-facing)
**Last updated**: 2025-12-29
**Audience**: Upstream dev lead / reviewers

Use this checklist to validate the downstream ingestion implementation and flag any issues.

---

## 1) CLI Surface Review

- [ ] `rzn-tools ingest sources` lists ingest-ready tools from `connectors/ingest_sources`.
- [ ] Filters work: `--connectors`, `--categories`, `--include-read`, `--include-fetch`.
- [ ] `rzn-tools ingest add` accepts `--args` JSON and stores per-tenant config.
- [ ] `rzn-tools ingest list` shows configured sources and last-run state.
- [ ] `rzn-tools ingest run` executes tools and writes normalized outputs.

---

## 2) Config & State

- [ ] Default tenant file: `~/.config/rzn-tools/ingest_sources.json`.
- [ ] Named tenant file: `~/.config/rzn-tools/tenants/<tenant>/ingest_sources.json`.
- [ ] Config persists:
  - `default_args`, `args`, `enabled`, `cadence_seconds`
  - `last_cursor`, `last_run_at`, `last_error`
- [ ] Cursor is persisted **only** from normalized outputs.

---

## 3) Normalized Output Compliance

- [ ] Ingest loop **forces** `output_format="normalized_v1"` on every call.
- [ ] Pageable tools return `type="rzn-tools.normalized_page.v1"` with top-level:
  - `next_cursor`
  - `has_more`
- [ ] Fetch tools return `type="rzn-tools.normalized_item.v1"`.

---

## 4) Indexing & Deduplication

- [ ] `items.jsonl` and `blocks.jsonl` are written per tenant under:
  - `~/.config/rzn-tools/ingest/` or `~/.config/rzn-tools/tenants/<tenant>/ingest/`
- [ ] Items deduped on `item_ref`, blocks deduped on `block_ref`.
- [ ] Records carry `source` + `partial` metadata.

---

## 5) Error Handling

- [ ] Missing `structured_content` yields a clear error message.
- [ ] Unsupported normalized `type` yields a clear error message.
- [ ] Per-source errors are recorded in `last_error` without stopping other sources.

---

## 6) Quick Manual Test (No Network)

```bash
# Validate CLI surfaces (no network needed)
rzn-tools ingest sources --output json
rzn-tools ingest list --output json
```

---

## 7) Optional Integration Checks (Network / Auth)

```bash
# Add + run a known public source
rzn-tools ingest add hackernews:list --args '{"story_type":"top","limit":10}'
rzn-tools ingest run --max-pages 1
```

---

## 8) Open Questions / Feedback

- [ ] Do you want per-source cadence enforcement in the scheduler?
- [ ] Should indexing include additional JSONL outputs (relationships / truncations)?
- [ ] Should errors retry with backoff, or be left to the caller?
