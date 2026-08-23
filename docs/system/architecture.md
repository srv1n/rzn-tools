---
subject: architecture
keywords: [design, layers, registry, data flow, output]
part_of: overview
describes: [rzn_tools_core/src/lib.rs, rzn_tools_core/src/resolver.rs, rzn_tools_core/src/ingest.rs, rzn_tools_cli/src, rzn_tools_mcp/src]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to change or review the system structure."
skip_when: "You only need to run a command or use one connector."
---

# Architecture

The project has one shared engine and two ways to use it. The command-line
program and the server both use the same source adapters.

## Main parts

| Part | Code | Job |
| --- | --- | --- |
| Core | `rzn_tools_core/src/` | Defines connectors, auth, routing, output contracts, usage, and shared MCP behavior. |
| CLI | `rzn_tools_cli/src/` | Defines `rzn-tools`, command wrappers, setup, ingest, usage, and asset commands. |
| MCP | `rzn_tools_mcp/src/` | Runs the tool registry over stdio or HTTP. |

The connector contract is the `Connector` trait in `rzn_tools_core/src/lib.rs`.
A connector gives its name, tool schemas, auth rules, optional URL metadata,
and a tool-call handler. `ProviderRegistry` stores each connector behind a
shared async mutex. It can also store aliases.

## Request flow

```text
CLI command or MCP tools/call
  -> feature-enabled registry
  -> connector and tool selection
  -> stored profile auth and/or environment values
  -> provider API, browser, or local source
  -> structured result
  -> CLI envelope or MCP response
```

The CLI creates its registry in `rzn_tools_cli/src/commands/list.rs`. It loads
the selected auth profile and can wrap connectors with usage metering. The MCP
binary builds the same core registry in `rzn_tools_mcp/src/lib.rs`.

## Routing and search

`SmartResolver` is in `rzn_tools_core/src/resolver.rs`. It uses its own ordered
regular-expression table. It does not build that table from
`Connector::url_patterns()`. URL patterns from a connector are metadata for
tool discovery. Keep both surfaces in sync when a URL rule changes.

The CLI `fetch` command uses the resolver. If more than one result is close in
priority, pretty output can ask the user to choose. Machine output selects the
highest-priority result.

Federated search is a CLI search engine in `rzn_tools_core/src/federated.rs`.
It calls several connectors and merges the results. The `federated` connector
module is not added by `build_registry_enabled_only`, so it is not a normal MCP
connector in the standard binaries.

## Output contracts

There are two separate output choices:

- CLI `--output` selects the outer display: pretty, JSON, YAML, text, or Markdown.
- Connector `output_format` can request `raw`, `normalized_v1`, or `display_v1`.

The normalized contracts are in `rzn_tools_core/src/ingest.rs`. They use the
type names `rzn-tools.normalized_page.v1` and `rzn-tools.normalized_item.v1`.
The display contracts are in `rzn_tools_core/src/display/v1.rs`. MCP can ask a
connector for normalized data and convert it to display data.

Not every connector supports every connector output format. Check the tool
schema. Do not assume that a raw provider response is normalized.

## Feature boundary

Cargo features decide which connectors exist in a binary. The registry adds a
connector only when its feature is enabled. The important profiles are in
`rzn_tools_core/Cargo.toml`:

- `server-full` is the portable server set.
- `all-connectors` adds Telegram and macOS connectors.
- `desktop-full` also adds browser-cookie import and `x-browser`.

Discord is a separate feature and is not in `server-full`. A compiled module
can still be absent from the standard registry. Check both the Cargo feature
and `build_registry_enabled_only`.

## State

The project writes several kinds of local state:

- auth profiles: the platform config directory, under `rzn-tools/auth.json`
- CLI serve config: the same config area
- ingest sources and indexes: the config area under `rzn-tools/`
- usage events: `~/.rzn-tools/usage.jsonl`
- managed assets: the platform data directory under `rzn-tools/assets`

The auth JSON file is permission-limited on Unix, but it is not encrypted.
