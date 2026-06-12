# Web Search Tools — Spec and Integration Guide

This document lists every web search tool added in this branch, with precise inputs, outputs, auth, build flags, and usage examples. All tools follow the same design principles:

- Unified inputs and output shapes to reduce agent confusion
- Concise responses by default; raw payload only when requested
- Strict schemas (unknown fields rejected) for fast failure and better tool learning
- Date/locale/domain filters supported uniformly (mapped natively where possible)

See also: docs/FEATURES.md (build flags), docs/auth/README.md (env vars), docs/integrations/LLM_WEB_SEARCH.md (overview and rationale).

## Common Contract (applies to all)

- Tool name: `search`
- Default behavior: returns a grounded answer with citations (LLM providers) or a structured results list (SERP/crawl providers); concise output
- Base inputs (all optional unless marked required):
  - `query` (string, required): natural‑language question. Do not append years unless the user asked.
  - `max_results` (integer): approx number of sources/items to return. Aliases: `limit`, `num` are accepted.
  - `response_format` ("concise" | "detailed", default "concise"): when `detailed`, a `raw` payload is added.
  - `language` (string): BCP‑47 (e.g., "en").
  - `region` (string): Country/region code (e.g., "US").
  - `since` / `until` (string): "YYYY-MM-DD".
  - `date_preset` (string): "last_24_hours" | "last_7_days" | "last_30_days" | "this_month" | "past_year"; derives since/until when omitted.
  - `locale` (string): e.g., "en-US". Parsed into language/region when not explicitly provided.
  - `include_domains` / `exclude_domains` (string[]): domain allow/deny lists.
- Output fields (typical):
  - `provider` (string)
  - `model` (string) — LLM providers
  - `query` (string)
  - `limit_hint` (integer) — most providers
  - `answer` (string) — when provider composes one
  - `citations` (array) — when provider returns them
  - `results` (array) — SERP/crawl providers
  - `raw` (object) — only when `response_format="detailed"`

Notes
- Native filter mappings: SerpAPI/Serper map language/region→`hl`/`gl`; xAI supports web allow/block domains and X date filters; Exa supports include/exclude.
- When a provider lacks native parameters, filters are added to the model instruction for guidance (keeps contract uniform while remaining effective).

---

## LLM Provider Tools

### openai-search/search
- Feature: `openai-search`
- Auth: `OPENAI_API_KEY` (Bearer). Optional: `OPENAI_ORG_ID`, `OPENAI_PROJECT_ID`.
- Inputs: base inputs + `model` (string), `max_output_tokens` (integer)
- Output: `{ provider:"openai", model, query, limit_hint, answer, citations, raw? }`
- Example (CLI):
  - `rzn-tools openai-search search --query "What changed in SEC climate rules in 2025?" --max-results 4`
- Example (JSON-RPC):
  - `{ "method":"tools/call", "params": { "name":"openai-search/search", "arguments": { "query":"…", "max_results":4 } } }`

### anthropic-search/search (Claude Web Search)
- Feature: `anthropic-search`
- Auth: `ANTHROPIC_API_KEY` (`x-api-key`).
- Inputs: base inputs + `model`, `max_output_tokens`
- Output: `{ provider:"anthropic", model, query, limit_hint, answer, citations, raw? }`

### gemini-search/search (Google Search grounding)
- Feature: `gemini-search`
- Auth: `GEMINI_API_KEY` or `GOOGLE_API_KEY`.
- Inputs: base inputs + `model`
- Output: `{ provider:"google-gemini", model, query, limit_hint, answer, raw? }`

### perplexity-search/search (online browsing)
- Feature: `perplexity-search`
- Auth: `PPLX_API_KEY` (Bearer).
- Inputs: base inputs + `model`
- Output: `{ provider:"perplexity", model, query, limit_hint, answer, citations, raw? }`

### xai-search/search (xAI Responses API tools)
- Feature: `xai-search`
- Auth: `XAI_API_KEY` (Bearer).
- Inputs: base inputs + `sources` (array: "web" | "x"), `mode` ("auto"|"on"|"off"), `model`
- Notes:
  - `sources=["web"]` maps to `web_search`.
  - `sources=["x"]` maps to `x_search`.
  - `include_domains` / `exclude_domains` map to `web_search.filters.allowed_websites` / `blocked_websites`.
  - `since` / `until` map to `x_search.from_date` / `to_date` for X search.
- Output: `{ provider:"xai", model, query, sources, limit_hint, answer, citations, raw? }`

---

## SERP / Crawl Tools

### exa-search/search (Exa.ai)
- Feature: `exa-search`
- Auth: `EXA_API_KEY` (x-api-key).
- Inputs: base inputs + `livecrawl` (bool), `include_text` (bool); domain allow/deny are native.
- Output: `{ provider:"exa", query, limit_hint, livecrawl, results, raw? }`

### firecrawl-search/search (Firecrawl v2)
- Feature: `firecrawl-search`
- Auth: `FIRECRAWL_API_KEY` (Bearer).
- Inputs: base inputs + `sources` (array: "web" | "images" | "news"), `scrape` (bool)
- Output: `{ provider:"firecrawl", query, sources, limit_hint, results, raw? }`

### serper-search/search (Serper.dev)
- Feature: `serper-search`
- Auth: `SERPER_API_KEY` (X-API-KEY).
- Inputs: base inputs (language/region mapped to `hl`/`gl` internally)
- Output: `{ provider:"serper", query, num, results, raw? }`

### serpapi-search/search (SerpAPI)
- Feature: `serpapi-search`
- Auth: `SERPAPI_API_KEY` (query param `api_key`).
- Inputs: base inputs + `engine` (default "google"); `hl`/`gl` accepted; `locale` and base `language`/`region` fold into `hl`/`gl` if not set.
- Output: `{ provider:"serpapi", query, num, engine, results, raw? }`

### tavily-search/search (Tavily)
- Feature: `tavily-search`
- Auth: `TAVILY_API_KEY` (body api_key).
- Inputs: base inputs + `topic` ("general"|"news"), `depth` ("basic"|"advanced"), `include_answer` (bool), `include_images` (bool)
- Output: `{ provider:"tavily", query, topic, depth, max_results, answer, results, raw? }`

---

## Best Practices & Optimizations

- Prefer `response_format:"concise"` (default). Use `detailed` only for debugging/auditing to avoid large payloads.
- Keep `max_results` small (3–8) to reduce tokens and improve precision.
- For newsy queries, set `date_preset:"last_7_days"` (or explicit `since`/`until`).
- Use `locale` (e.g., "en-US") instead of manually setting `language` + `region`.
- Curate sources via `include_domains` where possible for higher‑quality citations.
- Unknown params are rejected; fix by renaming to the unified field names.

---

## CLI Examples (copy/paste)

- OpenAI concise answer:
  - `rzn-tools openai-search search --query "What changed in SEC climate rules in 2025?" --max-results 4`
- xAI live search across web+X:
  - `rzn-tools xai-search search --query "Latest on OpenAI board changes" --max-results 6`
- SerpAPI localized SERP:
  - `rzn-tools serpapi-search search --query "best postgres connection pool settings" --max-results 10`
- Exa neural search:
  - `rzn-tools exa search --query "rust async runtime best practices" --num-results 8`
- Perplexity search:
  - `rzn-tools perplexity-search search --query "AI safety research 2025"`

Use `rzn-tools <connector> --help` for all available options.

---

## JSON‑RPC Examples (MCP)

- OpenAI concise:
{"method":"tools/call","params":{"name":"openai-search/search","arguments":{"query":"What changed in SEC climate rules in 2025?","max_results":4}}}

- Exa livecrawl:
{"method":"tools/call","params":{"name":"exa-search/search","arguments":{"query":"rust async runtime best practices","livecrawl":true,"max_results":8}}}

---

## Build Flags Recap

- Enable connectors individually on CLI/MCP:
  - `cargo build -p rzn_tools_cli --features "openai-search,serpapi-search"`
- macOS automation (macOS):
  - `cargo build -p rzn_tools_core --features macos-automation`

---

## Auth Quick Reference

- OpenAI: `OPENAI_API_KEY` (+ `OPENAI_ORG_ID`, `OPENAI_PROJECT_ID`)
- Anthropic: `ANTHROPIC_API_KEY`
- Gemini: `GEMINI_API_KEY` or `GOOGLE_API_KEY`
- Perplexity: `PPLX_API_KEY`
- xAI: `XAI_API_KEY`
- Exa: `EXA_API_KEY`
- Firecrawl: `FIRECRAWL_API_KEY`
- Serper: `SERPER_API_KEY`
- SerpAPI: `SERPAPI_API_KEY`
- Tavily: `TAVILY_API_KEY`

---

## Troubleshooting

- Unknown field error → remove or rename to the unified parameter names.
- No results vs answer only → providers vary: LLM tools return `answer` and `citations`; SERP tools return `results`. Use `response_format:"detailed"` to inspect `raw` for provider nuances.
- Auth errors → export the env var or run `rzn-tools config set <connector> --value "<api_key>"`.
