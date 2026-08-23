---
title: "System overview"
subject: overview
keywords: [system, map, rzn-tools]
part_of:
describes: [README.md, Cargo.toml, rzn_tools_core, rzn_tools_cli, rzn_tools_mcp]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ 78177bcbe092437637fb6fb55edd316c41972457
read_when: "You need the short map of this repository."
skip_when: "You need a command, connector, or build rule. Open the matching page."
---

# System overview

This project is a Rust program that gives one interface to many data sources.
It has three main parts.

| Part | Path | Job |
| --- | --- | --- |
| Core library | `rzn_tools_core/` | Defines connectors, auth, routing, output, and MCP logic. |
| CLI | `rzn_tools_cli/` | Provides the `rzn-tools` command. |
| MCP server | `rzn_tools_mcp/` | Serves the same tools over MCP stdio or HTTP. |

The normal data path is:

```text
user or agent
  -> CLI or MCP
  -> connector registry
  -> connector tool
  -> provider API or local source
  -> structured result
```

Cargo features decide which connectors are compiled. The default CLI build
contains a small useful set. The `full` profile adds the portable server set.
Desktop-only and browser features stay opt-in.

Start here:

- [Architecture](architecture.md) explains the code layout.
- [CLI](cli.md) explains common commands.
- [Connectors](connectors.md) lists the source adapters.
- [MCP](mcp.md) explains stdio and HTTP use.
- [Development](development.md) gives build and test commands.
- [Operations](operations.md) gives install and run steps.
- [Security](security.md) gives credential and privacy rules.

<!-- tusker:docs-map:begin -->
```mermaid
graph TD
  n_architecture["architecture"]
  n_cli["cli"]
  n_connectors["connectors"]
  n_development["development"]
  n_mcp["mcp"]
  n_operations["operations"]
  n_overview["System overview"]
  n_security["security"]
  n_overview --> n_architecture
  n_overview --> n_cli
  n_overview --> n_connectors
  n_overview --> n_development
  n_overview --> n_mcp
  n_overview --> n_operations
  n_overview --> n_security
```
<!-- tusker:docs-map:end -->
