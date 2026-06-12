# Connector Coverage Rollout (Metadata + Normalized Output v1)

**Status**: Draft (work plan + conformance checklist)
**Last updated**: 2025-12-29
**Audience**: Connector authors, downstream hosts (desktop/server), reviewers
**Goal**: “add a connector” without downstream custom logic (discovery, auth UI, paging, or output parsing).

This document defines:
1) what “coverage” means for a connector,
2) what must be implemented to be considered compliant,
3) how to roll it out safely across all connectors.

Related specs:
- Normalization contract: `docs/integrations/NORMALIZATION_SPEC_V1.md`
- Background + examples: `docs/integrations/INGEST_CONTRACT_V1.md`
- Launcher discovery: `docs/integrations/LAUNCHER_INTEGRATION_SPEC.md`

---

## 0) Principle: Compile-Time Enables, Runtime Selects

### Compile-time

Connectors are included by Cargo features. Downstream hosts should rely on:
- `build_registry_enabled_only()` (from `rzn_tools_core/src/lib.rs`) to register connectors from the
  compiled feature set.

This ensures the server “broadcast surface” is driven by compilation flags, not host hardcoding.

### Runtime

Hosts (desktop/server/tenant) decide which connectors/tools are actually enabled at runtime:
- tenant allowlist / user toggles / admin policy
- can filter by connector name, tool name, and `_meta` tags

**Do not** bake per-connector logic into the host. The host should be generic:
- discover via `connectors/list` + `tools/list`
- call tools with canonical params and `output_format="normalized_v1"` for ingestion

---

## 1) What “Connector Coverage” Means (Definition of Done)

A connector is considered “covered” when it satisfies all of the following.

### 1.1 Discovery & UX metadata

Connector **MUST** implement these trait methods:
- `display_name()`
- `icon()` (ASCII identifier string; no emojis)
- `categories()`
- `requires_auth()`
- `url_patterns()` (empty vec is allowed, but preferred when the connector can be driven by URLs)

Connector must be discoverable via:
- `connectors/list` (MCP JSON-RPC method), including:
  - name/display_name/description/icon
  - categories/url_patterns
  - auth_required/auth_status

### 1.2 Canonical tool surface

Connector **SHOULD** expose a small canonical surface (even if legacy tools remain callable):
- `connector/search` (discovery)
- `connector/get` (fetch full content by URL/ID)
- `connector/list` (browse a feed/collection) when a feed concept exists

If the connector has multiple “modes” (top/new/hot), prefer parameters over many tools.

### 1.3 Standard inputs

Indexable tools **MUST** implement the base input contract:
- `output_format: "raw" | "normalized_v1"`
- plus `limit`/`cursor` for pageable tools

### 1.4 Normalized output (for indexable tools)

Indexable tools **MUST** support:
- `output_format="normalized_v1"`
- return normalized payloads in `structured_content` following
  `docs/integrations/NORMALIZATION_SPEC_V1.md`

### 1.5 Tool schema quality

Indexable tools that support normalized output **MUST** include:
- `examples` (non-empty)
- `_meta` with at least:
  - `category`, `tags`, `auth_required`
  - `supports_output_format`, `supports_cursor`

### 1.6 Tests

Connector must include test coverage that proves:
- `connectors/list` includes this connector and its metadata is serializable
- `tools/list` schemas include required fields (`output_format`, and paging inputs if supported)
- at least one normalized response shape round-trips (fixture or mocked)

Avoid live network calls in CI; prefer fixtures and mocks.

---

## 2) Cross-Cutting Requirements (Invariants)

These invariants prevent downstream custom logic from creeping back in.

### 2.1 Pagination invariants

- Any tool that accepts `cursor` **MUST** return `rzn-tools.normalized_page.v1`.
- `next_cursor`/`has_more` must be top-level in the page object.

### 2.2 Identifier invariants

- `item_ref` and `block_ref` must be stable and unique within connector namespace.
- If synthesizing IDs, they must be deterministic (and recorded via metadata).

### 2.3 Auth probing invariants

`connectors/list` should be safe and fast:
- It **MUST NOT** trigger OS permission prompts or personal-data access.
- It **SHOULD NOT** perform heavy network probes by default for every connector.

Recommended behavior (for rollout):
- Keep the existing enum (`not_required|needs_setup|ready|invalid|unknown`).
- Compute `needs_setup` using local credential presence (no network).
- Only set `ready/invalid` if an explicit auth probe is requested (see “Auth probing plan” below).

---

## 3) Rollout Phases (Recommended)

### Phase A: Public / low-risk connectors (fast wins)

Targets:
- `wikipedia`, `arxiv`, `biorxiv`, `rss`, `web`, `hackernews`, `reddit`, `youtube`, `pubmed`

Work:
- Add normalized outputs for `search/list/get` where applicable.
- Ensure cursor semantics match the normalization spec.
- Add examples + `_meta` for canonical tools.

### Phase B: API-key search connectors (indexable, but auth-required)

Targets:
- `exa_search`, `tavily_search`, `serpapi_search`, `serper_search`, `firecrawl_search`,
  `perplexity_search`, `openai_search`, `anthropic_search`, `gemini_search`, `xai_search`

Work:
- Normalize search results as `ContentItem` candidates:
  - blocks may be empty or contain a short snippet block
  - canonical_url should be set
  - metadata can include ranking scores, provider ids, etc.

### Phase C: Collaboration/dev connectors (auth, personal-ish but not OS prompts)

Targets:
- `github`, `slack`, `discord`, `atlassian`, `microsoft-graph` (Graph is high-risk; treat carefully)

Work:
- Normalize thread/message/document outputs.
- Prioritize stable IDs and safe truncation.

### Phase D: Personal-data / OS permission connectors (explicit user opt-in)

Targets:
- `apple-mail`, `apple-notes`, `apple-messages`, `apple-reminders`, `apple-contacts`, `imap`,
  `google-gmail`, `google-people`

Work:
- Normalization is valuable, but must be gated by explicit user permission.
- `connectors/list` must never prompt OS dialogs.

---

## 4) Connector Matrix (Current Inventory + Required Work)

This is the current connector inventory. Use it as a checklist, not a perfect taxonomy.

Legend:
- **Auth**: `true` means `requires_auth() == true`.
- **Indexable**: should support normalized output for at least one of list/search/get.
- **Probe-safe**: safe to call `test_auth()` during discovery without OS prompts or personal-data access.

| Connector | Auth | Indexable | Probe-safe | Notes |
|----------|------|-----------|-----------|------|
| anthropic_search | true | yes | yes | API key / provider auth |
| apple_contacts | true | yes | no | OS permission prompt risk |
| apple_health | true | maybe | no | Not ready on macOS per repo note |
| apple_mail | true | yes | no | OS permission prompt risk |
| apple_messages | true | yes | no | OS permission prompt risk |
| apple_notes | true | yes | no | OS permission prompt risk |
| apple_reminders | true | yes | no | OS permission prompt risk |
| arxiv | false | yes | yes | public |
| atlassian | true | yes | yes | OAuth/API token |
| biorxiv | false | yes | yes | public |
| discord | true | yes | yes | bot token (network probe ok) |
| exa_search | true | yes | yes | API key |
| firecrawl_search | true | yes | yes | API key |
| gemini_search | true | yes | yes | API key |
| github | true | yes | yes | device flow / PAT |
| google_calendar | true | yes | no* | avoid probing by default |
| google_drive | true | yes | no* | avoid probing by default |
| google_gmail | true | yes | no | personal-data |
| google_people | true | yes | no | personal-data |
| google_scholar | false | yes | yes | public/scrape |
| hackernews | false | yes | yes | public |
| imap | true | yes | no | personal-data |
| localfs | false | yes | yes | local only |
| macos | false | maybe | no* | may require automation permissions |
| microsoft (graph) | true | yes | no | personal-data / org data |
| openai_search | true | yes | yes | API key |
| parallel_search | true | yes | yes | aggregator (depends on sub-connectors) |
| perplexity_search | true | yes | yes | API key |
| pubmed | false | yes | yes | public |
| reddit | false | yes | yes | public (optional auth) |
| rss | false | yes | yes | public |
| scihub | false | yes | yes | open-access lookup (no paywall bypass) |
| semantic_scholar | true | yes | yes | API key |
| serpapi_search | true | yes | yes | API key |
| serper_search | true | yes | yes | API key |
| slack | true | yes | yes | OAuth token |
| spotlight | false | yes | no* | local indexing; may require permissions |
| tavily_search | true | yes | yes | API key |
| web | false | yes | yes | public (may be blocked by captcha) |
| wikipedia | false | yes | yes | public |
| x (twitter) | false | yes | yes | scraper-based; may be blocked |
| xai_search | true | yes | yes | API key |
| youtube | false | yes | yes | public |

Notes:
- “Probe-safe” `no*` means “avoid probing during discovery by default” (even if it wouldn’t pop OS
  dialogs, it may be slow/side-effectful).

---

## 5) Auth Probing Plan (Recommended Update)

### Problem

`connectors/list` is primarily for discovery/UX. If it calls `test_auth()` for every connector, it:
- may trigger OS permission prompts,
- may create slow startup time (dozens of network calls),
- may cause rate limits / noisy logs.

### Recommended solution

Add an optional parameter to `connectors/list`:
- `probe_auth?: boolean` (default: false)

Behavior:
- If `probe_auth=false`:
  - `auth_status = not_required` when `requires_auth=false`
  - `auth_status = needs_setup` when auth is required but no credentials are present
  - `auth_status = unknown` when credentials exist but were not probed
- If `probe_auth=true`:
  - For probe-safe connectors only, call `test_auth()` and map to `ready|invalid|unknown`
  - For personal-data connectors, always return `unknown` unless user explicitly requests a probe

This keeps discovery deterministic and makes “auth verification” an explicit action.

---

## 6) Optional: “Ingestion Sources” (How hosts ingest without per-connector logic)

### Problem

Even with normalized outputs, ingestion still needs a “seed”:
- Which subreddits? Which channels? Which folders? Which queries?

If hosts have to hardcode these per connector, you reintroduce downstream custom code.

### Implemented approach (recommended)

rzn-tools implements a dedicated MCP method:
- `connectors/ingest_sources`

Spec + params:
- `docs/integrations/INGEST_SOURCES_ENDPOINT_SPEC.md`

Each ingest source declares:
- `id`, `display_name`, `description`
- `tool` to call (`connector/list` or `connector/search`)
- `input_schema` for configuration (e.g., subreddit list, channel id, folder id)
- `default_args` (e.g., `limit`, default sort, default window size)

Hosts then implement one generic ingestion loop:
1) list ingest sources
2) admin configures sources (per tenant)
3) scheduler runs the declared tool with declared defaults + stored cursor
4) host calls `connector/get` for selected items as needed

This keeps ingestion scalable without hardcoding connector semantics in hosts.

---

## 7) Checklist: Adding or Upgrading a Connector

1) Discovery metadata
   - Implement `display_name/icon/categories/requires_auth/url_patterns`
2) Canonical tool surface
   - Ensure `search/get/list` exist where applicable; keep legacy callable if needed
3) Standard inputs
   - Add `output_format`, and `limit/cursor` for pageable tools (schema + runtime parsing)
4) Normalized outputs
   - Implement `normalized_v1` outputs for indexable tools
   - Ensure cursor placement invariants
5) `_meta` + examples
   - Add `examples` and `_meta` fields for canonical tools
6) Tests + docs
   - Add conformance tests (no live network in CI)
   - Update changelog + relevant integration docs
