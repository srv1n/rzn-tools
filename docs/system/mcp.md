---
subject: mcp
keywords: [model context protocol, server, stdio, http]
part_of: overview
describes: [rzn_tools_mcp/src, rzn_tools_core/src/mcp_server.rs, rzn_tools_cli/src/commands/serve.rs]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to run or configure the MCP server."
skip_when: "You need normal CLI commands. Open cli.md."
---

# MCP server

The server lets an MCP client use the project tools through a local process or
an HTTP endpoint.

The binary is `rzn-tools-mcp`. It exposes compiled connector tools over stdio
or HTTP. Its default feature set has no connectors. Build a connector profile
or an explicit connector feature.

## Stdio

Use stdio when an MCP client starts the server:

```bash
make run CARGO_ARGS="-p rzn_tools_mcp --features full --"
```

## HTTP

```bash
make run CARGO_ARGS="-p rzn_tools_mcp --features full -- http --bind 127.0.0.1:8000"
```

The routes are `/`, `/mcp`, `/healthz`, and `/readyz`. `/mcp` accepts JSON-RPC
POST requests. After initialize, requests need the returned `mcp-session-id`.
DELETE closes a session.

HTTP GET on `/mcp` does not provide a long-lived SSE stream. A POST that asks
for `text/event-stream` gets one SSE event. Do not describe this server as a
general GET-SSE transport.

## Configuration

| Value | Use |
| --- | --- |
| `RZN_TOOLS_MCP_BIND` | Full bind address. |
| `PORT` | Port on `127.0.0.1` when no full bind is set. |
| `ALLOWED_HOSTS` | Comma-separated exact host allowlist. |
| `RZN_TOOLS_MCP_CONNECTORS` | Comma-separated connector allowlist. |
| `--all-connectors` | Remove the connector allowlist. |

The host allowlist is optional. When set, a bad `Host` header gets HTTP 421 on
the MCP routes. Health and readiness routes do not use this check. The server
has no built-in HTTP authentication, TLS, or rate limit. Keep it on localhost
or place it behind a trusted authenticated proxy.

## CLI service command

`rzn-tools serve` is a separate CLI front end. It exists only when the CLI is
built with `serve` or `full`. Its initial connector allowlist is `youtube`,
`hackernews`, `pubmed`, and `reddit`. `--connectors` replaces this list;
`--all-connectors` removes it.

One-time `--bind` and `--allow-hosts` values apply to that process. Connector
list edits and `configure cloudflare tunnel` write the serve config. Do not
assume every command-line override is saved.

## Tool names

The shared MCP layer publishes tools as `connector/tool`. It also publishes
auth helper tools on the normal MCP catalog. The HTTP catalog rewrites `/` to
`.` and hides auth and legacy-alias entries.

Tool calls accept slash or dotted names. Runtime aliases can work even when
they do not appear in `tools/list`. Use the listed canonical name in new code.

`rzn_tools_mcp/mcp_config.example.toml` is an old example. The current binary
does not read that TOML file. Use command arguments and environment values.
