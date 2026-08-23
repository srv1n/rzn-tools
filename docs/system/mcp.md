---
title: "MCP"
subject: mcp
keywords: [model context protocol, stdio, http, json-rpc]
part_of: overview
describes: [rzn_tools_mcp/src, rzn_tools_core/src/mcp_server.rs, rzn_tools_cli/src/commands/serve.rs]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to run or configure the MCP server."
skip_when: "You need normal CLI commands."
---

# MCP

The server lets an AI client call project tools.

Its binary name is `rzn-tools-mcp`. It uses standard input by default. It can also use HTTP.

The MCP crate has no connectors in its default feature set. Select `server-full` or specific connector features.

## Standard input

```bash
make run CARGO_ARGS="-p rzn_tools_mcp --features server-full --"
```

Use this mode when an MCP client starts the process.

## HTTP

```bash
make run CARGO_ARGS="-p rzn_tools_mcp --features server-full -- http --bind 127.0.0.1:8000"
```

The HTTP server has these routes:

| Route | Method | Purpose |
| --- | --- | --- |
| `/` and `/mcp` | POST | JSON-RPC request. |
| `/` and `/mcp` | DELETE | Close an MCP session. |
| `/healthz` | GET | Process health. |
| `/readyz` | GET | Initialization state and bind address. |

`initialize` returns `mcp-session-id`. Later requests must send this header.

GET does not open an MCP event stream. A POST can request `text/event-stream`. The server then returns one event.

## Configuration

| Setting | Purpose |
| --- | --- |
| `RZN_TOOLS_MCP_BIND` | Full bind address. |
| `BIND` | Secondary bind setting. |
| `PORT` | Port on `127.0.0.1`. |
| `ALLOWED_HOSTS` | Comma-separated exact host allowlist. |
| `RZN_TOOLS_MCP_CONNECTORS` | Comma-separated connector allowlist. |
| `--all-connectors` | Disable the connector allowlist. |

The precedence is `RZN_TOOLS_MCP_BIND`, `BIND`, `PORT`, then `127.0.0.1:8000`.

The server does not read a TOML config file.

## Tool names

The shared MCP layer uses `connector/tool`. The HTTP catalog presents the same tool as `connector.tool`.

Use the name from `tools/list`. The server does not provide alternate connector or tool names.

The shared server also provides auth helper tools. The HTTP catalog lists them with dotted names.

## Methods

The server implements standard resource, prompt, and tool methods. It also implements these project methods:

- `connectors/list`
- `connectors/ingest_sources`
- `authorization/describe`
- `authorization/status`
- `secrets/set`
- `secrets/delete`

These methods can read or change local auth state. Restrict access to the server.

## CLI service command

The CLI `serve` command exists when the CLI has the `serve` feature. Its default allowlist is `youtube`, `hackernews`, `pubmed`, and `reddit`.

Use `--connectors` to replace this list. Use `--all-connectors` to expose all compiled connectors.

The CLI stores serve settings in `serve.json` beside the auth store. One-time bind and host options do not all persist.
