---
schema: "tusker.domain-canon/v7"
kind: "domain_canon"
id: "project/canon"
project: "rzn-tools"
domain: "project"
title: "Project Canon"
status: "current"
summary: "Current durable truth for the rzn-tools repository."
capsule:
  what: "Current durable truth for the rzn-tools repository."
  use_when: "Use before changing code, commands, connectors, or docs."
  skip_when: "Skip when you only need task proof or generated tracker views."
source_of_truth:
  - "knowledge/domains/project/CANON.md"
created_at: "2026-08-23T10:54:54Z"
updated_at: "2026-08-23T10:54:54Z"
state_rev: "sha256:b5110e1049c511c57d2e40f18c5e607bf91419820f82b135ce933ca5dc4ae381"
---

# Project Canon

## Current Truth

- `rzn-tools` is a Rust workspace with a core library, a CLI, and an MCP server.
- Connectors are feature-gated and share one `Connector` trait and registry.
- The CLI and MCP server build their tools from the same core registry.
- Each connector and tool has one public name.
- Canonical system documentation lives in `docs/system/`.
- `normalized_v1` and `display_v1` are current wire values defined in code.
- Credentials belong in the local auth store or environment values, never in source.

## Stable Interfaces

- `rzn_tools_core::Connector` is the connector interface.
- `rzn-tools` is the CLI binary.
- `rzn-tools-mcp` is the MCP server binary.
- `make` targets are the supported build and test entry points.
- `rzn_tools_core/src/lib.rs` is the connector registry authority.
- `rzn_tools_cli/src/cli.rs` is the CLI grammar authority.
- `rzn_tools_core/src/mcp_server.rs` and `rzn_tools_mcp/src/http.rs` are the MCP authorities.

## Constraints

- Keep this canon short enough to read before implementation.
- Use the standard library or an existing helper before adding a dependency.
- Do not use personal-data connectors in automated tests.
- Do not expose an HTTP server without a host allowlist and a trusted network path.
- Delete obsolete project guidance. Git history keeps the record.

## Open Questions

- _None yet._
