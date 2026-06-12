# Tooling Guidelines for RZN Connectors

This document codifies conventions used across all connectors, adapted for LLM agents and aligned with Anthropic’s recommendations for “writing tools for agents”.

## 1) Tool Shape

- Small, composable tools: each tool performs one clear action (e.g., `list_messages`, `get_message`, `send_mail`).
- Deterministic and idempotent where possible. Side‑effects must support `dry_run=true` or a dedicated `*_preview` tool.
- Inputs/outputs: define JSON Schemas; all inputs validated before execution. Outputs include a machine‑readable object plus a short `text` summary.

## 2) Safety & Permissions

- Minimal scopes by default; read‑only first, then write tools behind explicit opt‑in.
- Surface provenance (source URL/IDs) and ACL metadata on items; use these in downstream authorization.
- PII handling: configurable redaction, file‑type allow‑list, size limits, and content sniffing before LLM exposure.

## 3) Reliability

- Timeouts per call; retry with exponential backoff on 429/5xx. Respect vendor rate‑limit headers.
- Typed errors with actionable `message` + `kind` + vendor `code`.
- Pagination: expose `next_cursor` consistently; hide vendor specifics.

## 4) User Experience

- Self‑describing tools: name, description, and parameter docs optimized for LLM selection.
- `test_auth` tool for quick validation; `initialize` returns capabilities and instructions.
- Log scrubbing: never log secrets or message bodies by default; opt‑in verbose tracing.

## 5) Development Patterns

- Auth abstraction: OAuth2/device‑code/service accounts/CLI tokens through the existing `AuthDetails`/`AuthStore` patterns.
- HTTP: `reqwest` client with middleware for headers, retries, and error mapping.
- Content processing: reuse existing chunkers, MIME sniffing, and HTML → text conversion utilities.

## 6) Testing

- Unit tests against recorded fixtures; smoke tests using sandbox tenants where feasible.
- Contract tests validate schemas and pagination cursors; rate‑limit behavior covered.

## 7) Telemetry

- Emit connector/tool names, latency, and status only (no payloads). Include vendor request IDs when present.

## Further Reading

- Anthropic Engineering — Writing tools for agents: https://www.anthropic.com/engineering/writing-tools-for-agents

