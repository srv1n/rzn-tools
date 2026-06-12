# Launcher Integration Review Checklist

## Code Review (Must‑Check)
- Connector trait defaults are present in `rzn_tools_core/src/lib.rs` and are non‑breaking.
- All connectors implement:
  `display_name`, `icon` (ASCII), `categories`, `requires_auth`, `url_patterns`.
- `connectors/list` endpoint:
  - Fields match spec (name/display_name/description/icon/tools_count/tools/categories/url_patterns/auth_required/auth_status).
  - Results are sorted by `display_name`.
  - Auth status mapping uses `test_auth()` and sensible fallbacks.
  - Personal‑data connectors are excluded from auth probing (see skip list in summary).
- Tool schemas (core connectors) include:
  - `examples` array with valid inputs
  - `_meta.category`, `_meta.tags`, `_meta.auth_required`
- No new emoji or non‑ASCII icon strings were introduced.
- `connectors/ingest_sources` endpoint:
  - Returns sources derived from tools with `_meta.supports_output_format=true` and `_meta.category`.
  - Default catalog excludes `resolve|download|export` and one-shot fetch tools (unless requested).
  - Supports filters (connector/category) per `docs/integrations/INGEST_SOURCES_ENDPOINT_SPEC.md`.

## Functional Expectations
- `connectors/list` is available via JSON‑RPC:
  `{ "method": "connectors/list", "params": {} }`
- `tools/list` now returns input schemas that include `examples` and `_meta` for core tools.
- Existing connectors still compile without adding metadata (defaults cover missing methods).

## Suggested Tests
1. `cargo test -p rzn_tools_core --tests`
2. `cargo test -p rzn_tools_core --tests --features "arxiv"`
3. Optional (feature‑scoped):
   `cargo test -p rzn_tools_core --tests --features "hackernews,youtube,reddit,wikipedia"`

## Questions / Decisions to Confirm
- Should auth probing be skipped for any additional connectors beyond the personal‑data list?
- Is `_meta.category` vocabulary acceptable (`search`, `read`, `list`, `resolve`)?
- Should non‑core connectors also receive examples immediately or staged later?
