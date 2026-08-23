---
title: "System overview"
subject: overview
keywords: [system, map, rzn-tools]
part_of:
describes: [README.md, Cargo.toml, rzn_tools_core, rzn_tools_cli, rzn_tools_mcp]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need a short map of this repository."
skip_when: "You need exact command or connector details."
---

# System overview

This project gives one interface to many data sources.

It is a Rust workspace named `rzn-tools`.

| Part | Path | Purpose |
| --- | --- | --- |
| Core | [`rzn_tools_core/`](../../rzn_tools_core/) | Defines connectors, routing, auth, output, usage, and shared MCP behavior. |
| CLI | [`rzn_tools_cli/`](../../rzn_tools_cli/) | Provides the `rzn-tools` command. |
| MCP | [`rzn_tools_mcp/`](../../rzn_tools_mcp/) | Provides MCP over standard input and HTTP. |

The normal request path is:

```text
caller -> CLI or MCP -> connector registry -> connector tool -> source -> result
```

Cargo features select the connectors in a binary. A connector is unavailable when its feature is off.

This directory is the complete product guide:

- [Architecture](architecture.md)
- [CLI](cli.md)
- [Connectors](connectors.md)
- [MCP](mcp.md)
- [Development](development.md)
- [Operations](operations.md)
- [Security](security.md)

The source code is the final authority. Use CLI help or MCP `tools/list` for the live schema.

<!-- tusker:docs-map:begin -->
```mermaid
graph TD
  n_architecture["Architecture"]
  n_cli["CLI"]
  n_connectors["Connectors"]
  n_development["Development"]
  n_mcp["MCP"]
  n_operations["Operations"]
  n_overview["System overview"]
  n_security["Security"]
  n_overview --> n_architecture
  n_overview --> n_cli
  n_overview --> n_connectors
  n_overview --> n_development
  n_overview --> n_mcp
  n_overview --> n_operations
  n_overview --> n_security
```
<!-- tusker:docs-map:end -->
