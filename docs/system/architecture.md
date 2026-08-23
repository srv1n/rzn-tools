---
title: "Architecture"
subject: architecture
keywords: [layers, registry, routing, output, state]
part_of: overview
describes: [rzn_tools_core/src/lib.rs, rzn_tools_core/src/resolver.rs, rzn_tools_core/src/ingest.rs, rzn_tools_cli/src, rzn_tools_mcp/src]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to change the system structure."
skip_when: "You only need to run a command."
---

# Architecture

The project has one engine and two front ends. The CLI and MCP server use the same connector code.

## Core contract

The [`Connector` trait](../../rzn_tools_core/src/lib.rs) defines each connector. A connector provides one name, tool schemas, auth metadata, and a call handler.

`ProviderRegistry` stores the enabled connectors. It wraps each connector in an async mutex.

[`build_registry_enabled_only`](../../rzn_tools_core/src/lib.rs) is the registry authority. Cargo features control its entries.

## Request flow

```text
CLI command or MCP tools/call
  -> enabled connector registry
  -> connector tool
  -> stored auth or connector environment data
  -> provider API or local source
  -> structured result
  -> CLI output or MCP response
```

The CLI creates the registry in [`commands/list.rs`](../../rzn_tools_cli/src/commands/list.rs). The MCP binary creates it in [`rzn_tools_mcp/src/lib.rs`](../../rzn_tools_mcp/src/lib.rs).

## Routing

[`SmartResolver`](../../rzn_tools_core/src/resolver.rs) maps known URLs and IDs to connector tools. Its rules are separate from connector URL metadata.

The CLI `fetch` command uses the resolver. A route can fail when its connector was not compiled.

Federated search is in [`rzn_tools_core/src/federated/`](../../rzn_tools_core/src/federated/). The CLI uses it when the caller selects a profile or sources.

## Output

Global `--output` selects CLI rendering. The values are `pretty`, `json`, `yaml`, `text`, and `markdown`.

Some tools accept `output_format`. The live values are `raw`, `normalized_v1`, and `display_v1`.

The `_v1` text is part of the current wire value. The running code requires the exact text.

- [`ingest.rs`](../../rzn_tools_core/src/ingest.rs) defines normalized records.
- [`display/`](../../rzn_tools_core/src/display/) defines display records and conversion.

Check the tool schema. Not every tool supports each value.

## Local state

| State | Code authority |
| --- | --- |
| Auth profiles | [`auth_store.rs`](../../rzn_tools_core/src/auth_store.rs) |
| CLI serve settings | [`commands/serve.rs`](../../rzn_tools_cli/src/commands/serve.rs) |
| Ingest state | [`commands/ingest.rs`](../../rzn_tools_cli/src/commands/ingest.rs) |
| Usage events | [`usage.rs`](../../rzn_tools_core/src/usage.rs) |
| Managed assets | [`paths.rs`](../../rzn_tools_core/src/paths.rs) |

The operating system selects config and data roots. Usage events use `~/.rzn-tools/usage.jsonl`.
