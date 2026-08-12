# Feature Flags and Platform Support

This crate compiles with a minimal core by default. You opt‑in to connectors and extras via Cargo features. This keeps builds small and enables cross‑platform targets (Windows, Linux, macOS, iOS, Android) without platform‑specific breakage.

## Distribution Profiles

- `server-full`: Portable API/HTTP connectors for server and downstream distributions. `web` and `rss` are opt-in rather than default-profile dependencies. It does not read local browser profiles, include Rookie or publicsuffix, or enable `x-browser`, `telegram`, Discord, or macOS-only connectors. Telegram remains locally available, but its upstream MTProto chain depends on a yanked crate and cannot resolve for a fresh downstream Cargo consumer.
- `desktop-full`: `all-connectors` plus `x-browser` and `browser-cookie-import` for an explicit, advanced local browser-profile import path. It includes the macOS connectors; Discord remains individually opt-in.
- `full`: Compatibility alias for the portable `server-full` profile. It is safe as a default/server distribution profile; `web`, `rss`, Discord, and browser-profile import remain explicit opt-ins. Use `desktop-full` only when browser-profile import is deliberately required.
- `all-connectors`: Convenience bundle that enables most connectors (use only when size isn’t a concern).
- examples: Enable example binaries under `rzn_tools_core/examples/*`.
- logging (suggested): Use tracing/tracing-subscriber in CLI/MCP (planned umbrella; currently enabled by default in those crates).

## Connectors (enable individually)

Academic & web:
- arxiv, pubmed, semantic-scholar, wikipedia, web, reddit, hackernews, youtube, polymarket, x (x-api), x-browser (x-twitter), scihub, imap, caldav, github, slack, atlassian

App stores:
- play-store, app-store, app-store-connect, apple-search-ads

Productivity:
- caldav, microsoft-graph, google-drive, google-gmail, google-calendar, google-people, linkedin

SEO / Search Console:
- google-search-console, bing-webmaster-tools

LLM provider web search:
- openai-search, anthropic-search, gemini-search, perplexity-search, xai-search

SERP / crawl APIs:
- exa-search, firecrawl-search, serper-search, serpapi-search, tavily-search

Platform specific and local-only:
- macos-automation (macOS only): Adds AppleScript/JXA via osakit. Safe on non‑mac targets (dependency and code are target‑gated), but functionality is a no‑op.
- browser-cookie-import: Advanced local opt-in to read cookies from installed browser profiles (uses Rookie + publicsuffix). It is not enabled by the portable/server profiles.
- browser-cookies: Compatibility alias for `browser-cookie-import`.

## Browser-session boundary

`rzn-tools` defaults to portable API/HTTP connectors. Web requests are cookie-free by default;
YouTube automatic browser-cookie import is opt-in; Reddit uses anonymous HTTP plus OAuth; and
the official `x` connector is the portable X route. For ordinary signed-in browser-session
workflows on Web, X, Reddit, or YouTube, use `rzn-browser`. The projects integrate at the host
or MCP-routing layer, not through a code dependency.

`x-browser` is an optional advanced/legacy connector. It can use explicitly supplied cookie
material; automatic browser-profile extraction requires `browser-cookie-import`.

## Build Recipes

- Minimal core:
  - `cargo build -p rzn_tools_core`
- CLI with a couple of connectors:
  - `cargo build -p rzn_tools_cli --features "openai-search,serpapi-search"`
- MCP server with productivity only:
  - `cargo build -p rzn_tools_mcp --features "microsoft-graph,google-drive"`
- Portable server distribution:
  - `make build CARGO_ARGS="-p rzn_tools_mcp --features server-full"`
- Desktop distribution with explicit browser-cookie import:
  - `make build CARGO_ARGS="-p rzn_tools_cli --features desktop-full"`
- macOS automation (macOS target):
  - `cargo build -p rzn_tools_core --features macos-automation`

## Cross‑Platform Notes (Tauri)

- macOS‑specific code is behind `#[cfg(target_os = "macos")]` and a feature flag; non‑mac targets compile clean.
- Avoid `all-connectors` in mobile builds; pick only what you need.
- Network/HTTP features (reqwest with `rustls-tls`) are already set for portable TLS.

## Auth and Environment Variables

- Provider auth is documented in `docs/auth/README.md` (OpenAI/Anthropic/Gemini/Perplexity/xAI, Exa/Firecrawl/Serper/Tavily/SerpAPI, etc.).
