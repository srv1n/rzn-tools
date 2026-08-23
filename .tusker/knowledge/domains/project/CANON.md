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
- Canonical system documentation lives in `docs/system/`.
- Credentials belong in the local auth store or environment values, never in source.

## Stable Interfaces

- `rzn_tools_core::Connector` is the connector interface.
- `rzn-tools` is the CLI binary.
- `rzn-tools-mcp` is the MCP server binary.
- `make` targets are the supported build and test entry points.

## Constraints

- Keep this canon short enough to read before implementation.
- Use the standard library or an existing helper before adding a dependency.
- Do not use personal-data connectors in automated tests.
- Do not expose an HTTP server without a host allowlist and a trusted network path.
- Move obsolete details to Deprecated Or Stale instead of deleting useful history.

## Deprecated Or Stale

- _None known._

## Open Questions

- _None yet._
