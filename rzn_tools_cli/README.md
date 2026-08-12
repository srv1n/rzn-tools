# `rzn-tools` CLI

The CLI is the shell surface for the RZN Integrations connectors. It keeps a small set of
stable commands instead of carrying one handwritten command tree for every connector.

## Install and verify

```bash
git clone https://github.com/srv1n/rzn-tools.git
cd rzn-tools
make build-release CARGO_ARGS="-p rzn_tools_cli"
./target/release/rzn-tools --help
```

## The supported command surface

### YouTube

YouTube has first-class search, video, playlist, channel, and transcript workflows:

```bash
rzn-tools youtube search --query "rust programming" --limit 10
rzn-tools youtube dQw4w9WgXcQ
rzn-tools yt dQw4w9WgXcQ
rzn-tools youtube transcript --id dQw4w9WgXcQ
```

The short transcript-friendly form is equivalent to `youtube <video-or-url>`.

### Smart search, get, and fetch

Use the generic commands when the connector or input is known at runtime:

```bash
rzn-tools search youtube "rust programming" --limit 5
rzn-tools get youtube dQw4w9WgXcQ
rzn-tools fetch https://www.youtube.com/watch?v=dQw4w9WgXcQ
rzn-tools fetch arXiv:2301.07041
rzn-tools fetch hn:38500000
```

`search <connector> <query>` and `get <connector> <id>` select one connector. `fetch <input>`
uses the smart resolver for supported URLs, IDs, and prefixes. Use `--output-format raw`,
`normalized_v1`, or `display_v1` with `fetch` when a specific shape is needed.

### Generic connector tools

Every connector exposes its actual tool names and argument schema through `tools`:

```bash
rzn-tools tools reddit
rzn-tools tools github
rzn-tools tools youtube
```

Call a tool with the JSON object shown by `tools`:

```bash
rzn-tools call github search_repositories \
  --args '{"query":"rust cli","limit":10}'
rzn-tools call reddit <tool-from-tools> \
  --args '<JSON_OBJECT_FROM_TOOLS>'
```

There are no connector-specific CLI subcommands for Reddit, GitHub, Slack, academic sources,
or other connectors. This keeps schemas in one place and prevents the CLI from drifting behind
connector capabilities.

### Listing, formats, and output

```bash
rzn-tools list
rzn-tools connectors
rzn-tools tools youtube
rzn-tools formats
rzn-tools --output json search youtube "rust"
rzn-tools --copy fetch hn:38500000
```

Global flags go before the command. `--output` supports `pretty`, `json`, `yaml`, `text`, and
`markdown` where the command supports that format. Use `-v` (or `-vv`) for diagnostics.

## Authentication and configuration

Use the generic setup and config commands for connectors that require credentials. Connector
docs describe provider-specific fields and scopes.

```bash
rzn-tools setup                 # discover available connector setup schemas
rzn-tools setup slack           # guided setup when the connector provides one
rzn-tools config show
rzn-tools config set github --value "ghp_your_token"
rzn-tools config test github
rzn-tools config remove github
```

For advanced schemas, set one field explicitly:

```bash
rzn-tools config set app-store-connect --key issuer_id --value "..."
```

Profiles are selected with `--auth-profile NAME` before the command:

```bash
rzn-tools --auth-profile work config show
rzn-tools --auth-profile work setup reddit
```

Environment variables remain supported where documented (for example `REDDIT_CLIENT_ID`,
`REDDIT_CLIENT_SECRET`, and provider API keys). Never commit credentials.

## Scripting and troubleshooting

Prefer JSON for scripts and inspect the schema before calling a connector:

```bash
rzn-tools --output json search youtube "rust" > results.json
rzn-tools tools <connector>
rzn-tools -vv get youtube dQw4w9WgXcQ
```

If a connector or tool is unavailable, start with `rzn-tools list` and `rzn-tools tools
<connector>`. For authentication failures, inspect `config show` and run `config test`.

The MCP server and Rust library use the same connector registry; this README documents only the
CLI surface.
