# RZN Integrations — dossier
Last updated: 2026-08-18 (update this line every edit)

## One paragraph
RZN Integrations (`rzn-tools`) is a local-first integration runtime: one command-line tool, MCP server, and Rust library for discovering and calling connectors to web services, SaaS APIs, public datasets, and local macOS/filesystem sources. It routes URLs and identifiers, searches one or several sources, stores connector credentials locally, and can return provider-native or shared normalized/display output.

## Status
Shipped and under active development. Public GitHub releases `v0.0.1` and `v0.0.2` exist; the latest release workflows passed on 2026-08-12. A `v0.0.2` arm64 macOS CLI is installed at `/Users/sarav/.local/bin/rzn-tools`; it reports package version `0.2.18` and is byte-identical to the public release asset. A local HTTP/Cloudflare serve profile exists but no `rzn-tools` process was running when inspected on 2026-08-18. No installed standalone `rzn-tools-mcp` binary was found. Real users: UNKNOWN — ask Sarav

## What it does (features, user-facing)
- Install one CLI and discover or call connector tools by namespace.
- Search a single source or federate a query across named connector sets.
- Route recognized URLs, local paths, DOIs, PMIDs, arXiv IDs, and social links to a connector.
- Fetch public research, web, social, app-store, prediction-market, RSS, weather, and video/transcript data.
- Connect authenticated GitHub, Slack, Google, Microsoft, Apple, LinkedIn, messaging, SEO, and search-provider accounts when the required feature and credentials are present.
- Run the connector catalog as an MCP server over stdio or HTTP, with an HTTP connector allowlist.
- Return provider-native data or shared `normalized_v1` / `display_v1` records.
- Configure credential profiles, inspect tools, meter invocation/cost estimates, and generate usage reports.
- Discover ingest-capable sources, save ingest configuration, and run normalized ingestion jobs.
- Install/update the bundled agent skill and inspect/sync workflow and quickstart assets.

## Who it's for
The documented audience is developers and AI-agent/app builders who need one local interface across multiple sources; README examples specifically cover shell use, MCP clients, Claude Code, Codex, ChatGPT, and Rust applications. Whether that matches today’s actual users: UNKNOWN — ask Sarav

## Numbers that are true
- Public GitHub snapshot on 2026-08-18: 1 star, 0 forks, 8 open issues, 1 contributor. Reproduce with `gh api repos/srv1n/rzn-tools` and `gh api repos/srv1n/rzn-tools/contributors`.
- GitHub traffic for the API’s current 14-day window: 104 clones by 41 unique cloners and 0 views. Reproduce with `gh api repos/srv1n/rzn-tools/traffic/clones` and `/traffic/views`; clones are not installs or users.
- GitHub release downloads: `v0.0.2` arm64 macOS CLI 1, workflow bundle 1, all other assets 0; `v0.0.1` checksums 2, arm64 macOS CLI 1, Linux CLI 2, other assets 0. Reproduce from each asset’s `download_count` via `gh api repos/srv1n/rzn-tools/releases`.
- Installed catalog: 47 connectors and 277 tools. Reproduce with the installed CLI’s JSON `list` output and count unique providers/tools.
- Installed bundled assets: 7 agent systems and 20 quickstarts, from `/Users/sarav/.local/share/rzn-tools`; reproduce by counting system and quickstart entries there.
- Local usage log through 2026-08-18: 6,670 invocations from 2026-04-14 onward; 5,634 `ok`, 1,036 `error` (84.47% / 15.53%), and $1.8134875 recorded estimated/provider cost. Reproduce by aggregating status and cost fields in `/Users/sarav/.rzn-tools/usage.jsonl`; these may include tests/manual runs and do not establish users or production traffic.
- Largest observed loss cases in that log: Reddit 959/2,105 errors (45.56%), X 28/34 (82.35%), YouTube 27/852 (3.17%). Reproduce by grouping the same log by connector and status.
- Package version: `0.2.18`; reproduce with `rg '^version = ' rzn_tools_{core,cli,mcp}/Cargo.toml`. Public release tag is `v0.0.2`, so tag and binary package versions do not match.
- Workspace: 3 crates. Reproduce from `[workspace].members` in root `Cargo.toml`.
- Revenue and paying-customer count: UNKNOWN — ask Sarav

## Tech shape (short)
- Rust 2021 workspace under AGPL-3.0-only, centered on Tokio, reqwest/rustls, serde, clap, and `rmcp`.
- `rzn_tools_core` owns connector contracts/registry, auth, routing, shared output, ingestion, MCP handling, and metering; CLI and MCP crates are adapters.
- Connectors are Cargo-feature gated; `server-full` is the portable server set, while browser-profile and macOS-native access are explicit desktop features.
- Credentials use a local JSON store with Unix mode `0600`; on this Mac the active path is under `~/Library/Application Support/rzn-tools`, not the `~/.config` path stated in README/SECURITY.
- HTTP MCP uses host/connector allowlists but has no client authentication; a Cloudflare tunnel can expose that surface.
- Release binaries are checksummed but the installer does not verify the checksum; the installed macOS binary is ad-hoc signed and rejected by Gatekeeper assessment.

## Recent changes (rolling, newest first, keep last ~10)
- 2026-08-18 observation, uncommitted: the dirty checkout contains Apple Ads Platform API v1 tools, optional CLI-side MCP wiring, and dependency/boilerplate removal.
- 2026-08-14, branch-only: immutable MCP sidecar packaging and process/frame-boundary tests were added in `eff7f9f`; this commit is not in the current checkout or `origin/main`.
- 2026-08-12, public `v0.0.2`: dead dependencies/vendor trees and connector CLI/runtime boilerplate were reduced; release and CI workflows passed.
- 2026-08-01, current branch history: `0.2.18` added portable `server-full`, desktop/browser-cookie profiles, agent-skill management, bundled assets, normalized ingestion, and expanded connectors.
- 2026-08-01: ordinary signed-in browser-session automation for YouTube, Reddit, Web, and X was assigned to the separate `rzn-browser` product.
- 2025-12-26, `0.2.15`: source builds gained a default connector set and improved missing-feature errors.
- 2025-12-24, `0.2.14`: connector-specific pagination and cursor arguments were exposed through the CLI.

## Deliberate exclusions
- Hosted SaaS operation is outside this repo; the documented boundary is a local runtime.
- Ordinary signed-in browser-session automation belongs to `rzn-browser`; automatic browser-cookie import is an explicit desktop opt-in.
- `server-full` excludes browser-profile import, `x-browser`, Telegram, Discord, and macOS-only connectors; Telegram is excluded because its dependency chain resolves through a yanked crate.
- Apple Health is not registered or feature-enabled because the macOS Health data store is unavailable; retained code is marked not ready.
- The Apple Ads raw v1 tool accepts relative JSON API requests, not binary asset uploads.
- Product capabilities deliberately refused by Sarav, and why: UNKNOWN — ask Sarav

## Open questions / embarrassments
- Product itch and triggering moment: UNKNOWN — ask Sarav
- What Sarav is proud of, and what Sarav personally considers embarrassing or unfinished: UNKNOWN — ask Sarav
- Actual users, their feedback, and whether the documented audience is accurate: UNKNOWN — ask Sarav
- Remote MCP HTTP has no client authentication. The remote setup guide knowingly exposes public HTTPS without an extra auth layer and only advises adding auth before wider sharing.
- Debug transport logging records raw JSON-RPC request lines and responses, defeating the MCP handler’s parameter redaction and potentially exposing credentials or personal connector results.
- `install.sh` recursively deletes a configurable asset directory without a broad-target guard, and installs downloaded release artifacts without checksum/signature verification.
- Several CLI truncation paths slice/truncate UTF-8 strings at byte offsets and can panic on non-ASCII input.
- `rzn-tools connectors` probes all connectors with live `test_auth()` calls even for machine output; PubMed prints timing lines to stdout, which made observed JSON output invalid. Its auth-required inference also mislabels credentialed connectors whose config can come from environment variables.
- TUI mode is a stub. Many connector docs are draft/future designs, while some implemented connectors remain labeled Draft.
- README’s authenticated Reddit row promises posting/private content, but current Reddit tools are read-only; docs elsewhere explicitly say private content is unavailable.
- SECURITY claims universal HTTPS, rate limiting, and input validation are not backed by universal enforcement; its supported-version table stops at `0.1.x` and its response-time promises have no repo evidence.
- Docs contain 42 direct `cargo build/test/run` references despite the repo policy requiring every Rust compilation through Make; MSRV `1.75.0` is declared but not tested in CI.
- CI tests default features, not all features; mutable GitHub Action tags are used instead of commit SHA pins.
- Local validation on 2026-08-18: format check, all-target/all-feature compile, default-feature workspace tests, and all-feature workspace docs with Rustdoc warnings denied passed. Strict all-target/all-feature Clippy failed on four `items_after_test_module` violations. All-feature tests passed 31/31 CLI and 144/146 core tests, failing two ordering assertions; that run stopped before later test binaries.
- The working tree had 85 tracked changed/deleted paths plus untracked task/plan files before this dossier edit; observed dirty-tree behavior is not a shipped release.
