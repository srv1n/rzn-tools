# CLI and MCP Reference

Use this when changing the command surface, MCP runtime, or agent-facing tool discovery.

## Contents

- [Public Surfaces](#public-surfaces)
- [Core Commands](#core-commands)
- [Command Design](#command-design)
- [MCP Server](#mcp-server)
- [LLM-Facing Tool Surface](#llm-facing-tool-surface)
- [Validation](#validation)

## Public Surfaces

| Surface | Binary/crate | Purpose |
|---|---|---|
| CLI | `rzn-tools` / `rzn_tools_cli` | Shell workflows, fetches, searches, setup |
| MCP server | `rzn-tools-mcp` / `rzn_tools_mcp` | Agent/client integration |
| Library | `rzn_tools_core` | Connector model, registry, auth, normalization |

The CLI and MCP server should expose the same connector capabilities through different UX layers. Do not fix one surface and leave the other unable to compile with the same feature.

## Core Commands

| Command | Purpose |
|---|---|
| `rzn-tools list` | Show available connectors |
| `rzn-tools tools [connector]` | Show MCP tools and auth requirements |
| `rzn-tools search ...` | Single-connector or federated search |
| `rzn-tools get <connector> <id>` | Fetch one item using a connector |
| `rzn-tools fetch <url-or-id>` | Smart resolver auto-routing |
| `rzn-tools setup [connector]` | Auth setup |
| `rzn-tools config ...` | Auth config show/set/test/remove |
| `rzn-tools serve ...` | Run native MCP HTTP server |
| `rzn-tools ingest ...` | Discover/configure/run ingestion sources |
| `rzn-tools pricing ...` | Pricing catalog lookup |
| `rzn-tools usage ...` | Usage totals |
| `rzn-tools workflows ...` | Starter workflow/example sync |
| `rzn-tools skills ...` | Install/update/remove the bundled repo Agent Skill |

Direct connector subcommands live in `rzn_tools_cli/src/cli.rs`. Examples include `youtube`, `hackernews`, `google-drive`, `google-search-console`, `microsoft-graph`, and `exa`.

## Command Design

Use direct flags for connector subcommands:

```bash
rzn-tools youtube search --query "rust programming" --limit 10
rzn-tools hackernews story --id 8863
rzn-tools google-drive list-files --page-size 20
```

Keep generic escape hatches working:

```bash
rzn-tools tools youtube --output json
rzn-tools search youtube "rust"
rzn-tools get youtube dQw4w9WgXcQ
rzn-tools fetch "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
```

For URLs containing `?`, quote the URL in shell examples.

## MCP Server

The server is launched by:

```bash
rzn-tools serve --local-only
rzn-tools serve --bind 127.0.0.1:9000
rzn-tools serve --connectors youtube,hackernews,pubmed
rzn-tools serve --all-connectors
```

Connector allowlisting matters for hosted/proxied use:

- `--connectors` replaces exposed connectors for a run.
- `--add-connectors` and `--remove-connectors` update persisted allowlists.
- `--all-connectors` exposes every compiled connector.
- `--local-only` disables Cloudflare tunnel auto-start.

## LLM-Facing Tool Surface

Tool discovery should stay small and predictable:

- concise responses by default,
- detailed payloads only on request,
- admin/watch/subscribe tools hidden unless `RZN_SHOW_ADMIN_TOOLS=1`,
- cursor tokens returned in concise responses when relevant,
- OAuth refresh handled when refresh tokens exist.

Macro tools are feature-gated with `llm-macros`. They combine multi-step flows such as finding/exporting Drive content or sending messages with attachments.

## Validation

Fast checks:

```bash
cargo check -p rzn_tools_cli
cargo check -p rzn_tools_mcp --features full
cargo run -p rzn_tools_cli -- list --output json
cargo run -p rzn_tools_cli -- tools --output json
cargo run -p rzn_tools_cli -- skills status --scope project --output json
```

Before release or broad CLI/MCP changes:

```bash
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```
