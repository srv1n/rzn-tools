# LLM Provider Web Search (Built-in Tools)

This adds uniform "search" tools that call LLM vendors’ native, built-in web search features where available. Use them like any other connector:

- List tools: `rzn tools openai-search` (or `anthropic-search`, `gemini-search`, `perplexity-search`)
- Run a search: `rzn search openai-search "what happened with …"`

All tools accept at minimum: `{ query: string, limit?: number, model?: string }`.

## Uniform Parameters (applies to all search tools)

- query: string — clear, natural-language question. Don’t append years unless the user asked.
- max_results: integer (default varies by provider; we set 3–10). Keeps outputs small and LLM-friendly.
- response_format: "concise" | "detailed" (default: concise)
  - concise: returns high-signal fields (answer, citations or results) and omits raw provider payload
  - detailed: includes the raw provider payload under `raw`
- language: string — BCP-47 hint, e.g., "en"
- region: string — country/region code, e.g., "US"
- since / until: string — date filters (YYYY-MM-DD)
- include_domains / exclude_domains: string[] — domain allow/deny lists

Notes:
- Some providers support locale/filters natively (e.g., SerpAPI hl/gl, xAI web filters + X date filters); we pass structured params where supported and otherwise fold them into the search instruction as guidance.
- Unknown fields are rejected (additionalProperties=false) so tools fail fast and teach agents correct usage.

## Providers and Capabilities

- OpenAI (Responses API): built-in `web_search` tool; model decides when to search and returns citations. Auth via `Authorization: Bearer OPENAI_API_KEY` (+ optional `OpenAI-Organization`, `OpenAI-Project`). Default model here: `o4-mini`.
- Anthropic (Claude Web Search): enable `tools: [{ type: "web_search_20250305" }]`; Claude browses automatically and returns citations. Auth via headers `x-api-key` and `anthropic-version: 2023-06-01`. Default model: `claude-3-7-sonnet-latest`.
- Google Gemini (Grounding with Google Search): enable the `googleSearch` tool in `tools` for Gemini models. Auth via API key (set `GEMINI_API_KEY` or `GOOGLE_API_KEY`). Default model: `gemini-1.5-pro-latest`.
- Perplexity (Search API): chat completions with `online` browsing; responses include citations. Auth via `Authorization: Bearer PPLX_API_KEY`. Default model: `sonar-pro`.
- xAI (Responses API tools): `POST /v1/responses` with built-in tools (`web_search`, `x_search`) and `search_mode` (`on`/`auto`/`off`). Auth via `Authorization: Bearer XAI_API_KEY`. Default model: `grok-4-fast`.

Third‑party SERP/crawl providers:

- Exa.ai: Fast web search with optional livecrawl and content extraction. Auth via `x-api-key: EXA_API_KEY`. Endpoint: `POST https://api.exa.ai/search`.
- Firecrawl: Unified search and scraping (web/images/news). Auth via `Authorization: Bearer FIRECRAWL_API_KEY`. Endpoint: `POST https://api.firecrawl.dev/v2/search`.
- Serper.dev: Google Search JSON API. Auth via `X-API-KEY: SERPER_API_KEY`. Endpoint: `POST https://google.serper.dev/search`.
- Tavily: Blended web/news search with summaries and citations. Auth via body `api_key: TAVILY_API_KEY`. Endpoint: `POST https://api.tavily.com/search`.
- SerpAPI: Google Search JSON with rich verticals/locality. Auth via query `api_key=SERPAPI_API_KEY`. Endpoint: `GET https://serpapi.com/search.json`.

Notes on other vendors:

- Mistral: Agents API supports a `web_search` tool; integration is planned.
- xAI (Grok): integrated via Responses API tools in this connector.
- Cohere: No native web search tool; they support tool use and connectors—you can bring your own search.

## Authentication and Config

You can set credentials via `rzn config set <connector>` or environment variables.

- OpenAI: `OPENAI_API_KEY` (required), `OPENAI_ORG_ID` (optional), `OPENAI_PROJECT_ID` (optional)
- Anthropic: `ANTHROPIC_API_KEY` (required)
- Gemini: `GEMINI_API_KEY` or `GOOGLE_API_KEY` (required)
- Perplexity: `PPLX_API_KEY` (required)
- xAI: `XAI_API_KEY` (required)
- Exa: `EXA_API_KEY` (required)
- Firecrawl: `FIRECRAWL_API_KEY` (required)
- Serper: `SERPER_API_KEY` (required)
- Tavily: `TAVILY_API_KEY` (required)
- SerpAPI: `SERPAPI_API_KEY` (required)

Each connector also allows an optional `model` override in config or per-call.

## Output Shape

All provider search tools return a structured payload:

```
{
  provider: "openai" | "anthropic" | "google-gemini" | "perplexity",
  model: string,
  query: string,
  limit_hint: number,
  answer: string,      // provider-formatted answer
  citations: [...],    // if provider exposes a top-level citations list
  raw: {...}           // full provider response for audit/debug
}
```

This keeps results machine-readable for MCP clients while preserving the raw payload.
