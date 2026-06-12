# RZN Integrations MCP Server (`rzn-tools-mcp`)

This is the MCP server for **RZN Integrations**. The `rzn-tools-mcp` binary exposes the repo's
connectors through a standardized protocol that any MCP-compatible client can call.

## Quick Start

### Running the MCP Server

```bash
# Run the MCP server with stdio transport
cargo run -p rzn_tools_mcp --bin rzn-tools-mcp

# Run the MCP server with native HTTP transport
cargo run -p rzn_tools_mcp --bin rzn-tools-mcp -- http --bind 127.0.0.1:8000

# Or build and run
cargo build --release -p rzn_tools_mcp --features full
./target/release/rzn-tools-mcp
```

`rzn-tools-mcp` now supports two transport modes:

| Mode | Command | Notes |
|---|---|---|
| `stdio` | `rzn-tools-mcp` | Best for local MCP desktop clients |
| `http` | `rzn-tools-mcp http --bind 127.0.0.1:8000` | Exposes `/mcp`, `/healthz`, `/readyz` for tunnels, workers, and remote proxies |

HTTP mode keeps a single MCP endpoint. There are no per-tool routes because that would be a bad design.

If you prefer the main CLI entrypoint, the same HTTP flow is available via:

```bash
rzn-tools configure cloudflare guide
rzn-tools configure cloudflare tunnel --hostname rzn-tools-origin.example.com --tunnel-name rzn-tools-mcp
rzn-tools serve

# Local-only mode if you do not want Cloudflare
rzn-tools serve --local-only
```

`cloudflared` is required for tunnel mode. `rzn-tools serve` will auto-start it when a tunnel name is configured. `wrangler` is only needed if you also deploy a Worker. Keep the hostname in `serve.json`, the hostname in `~/.cloudflared/config.yml`, and the `Host` header accepted by the running origin aligned, or Cloudflare will fail in ways that look random.

For the full operator walkthrough, see [`docs/integrations/REMOTE_MCP_SETUP.md`](../docs/integrations/REMOTE_MCP_SETUP.md).

### Environment Variables

Configure connectors by setting environment variables:

```bash
# Google Search
export GOOGLE_API_KEY="your_api_key"
export GOOGLE_CSE_ID="your_cse_id"

# Reddit
export REDDIT_CLIENT_ID="your_client_id"
export REDDIT_CLIENT_SECRET="your_client_secret"

# Brave Search
export BRAVE_API_KEY="your_api_key"

# X / Twitter
export X_BEARER_TOKEN="your_bearer_token"
export TWITTER_BEARER_TOKEN="your_bearer_token"
export X_OAUTH2_ACCESS_TOKEN="your_access_token"
export X_OAUTH2_REFRESH_TOKEN="your_refresh_token"
export X_OAUTH2_EXPIRES_AT="1767225599"
export X_OAUTH2_SCOPE="tweet.read users.read offline.access"
export X_OAUTH2_TOKEN_TYPE="bearer"
export X_CLIENT_ID="your_client_id"
export X_CLIENT_SECRET="your_client_secret"
export X_REDIRECT_URI="http://localhost:3000/callback"
export X_OAUTH_CONSUMER_KEY="your_consumer_key"
export X_OAUTH_CONSUMER_SECRET="your_consumer_secret"
export X_OAUTH_ACCESS_TOKEN="your_access_token"
export X_OAUTH_ACCESS_TOKEN_SECRET="your_access_token_secret"

# Then run the server
cargo run -p rzn_tools_mcp --bin rzn-tools-mcp
```

### X Connector Setup

The `x` connector uses the same auth model in CLI and MCP mode.

- Use a bearer token for public reads.
- Use OAuth 2.0 PKCE tokens for user-context reads and writes.
- Keep OAuth 1.0a only for imported legacy tokens or endpoint fallback.

For CLI-led setup, run `rzn-tools setup x` and import the credential family you actually have. For
MCP/tool-calling users, pass the same values through the server environment or config store, then
verify with `x/get_auth_status` and `x/whoami`.

## MCP Protocol Compliance

### Supported Capabilities

The server exposes the following MCP capabilities:

- **Tools**: Execute actions through connectors (search, fetch data, etc.)
- **Resources**: Access structured data from various sources
- **Prompts**: Use predefined prompt templates

### Available Connectors

**No Authentication Required:**
- `hackernews` - Search and fetch Hacker News stories
- `wikipedia` - Search and fetch Wikipedia articles
- `arxiv` - Search academic papers on arXiv
- `pubmed` - Search medical literature on PubMed
- `semantic_scholar` - Search academic papers on Semantic Scholar
- `web` - Basic web scraping

**Authentication Required:**
- `x` - X (Twitter) API (Credentials or browser cookies)
- `linkedin` - LinkedIn official OAuth/OIDC APIs (externally obtained token import only)
- `slack` - Slack API (Bot token)
- `github` - GitHub API (Personal access token)
- LLM search connectors (`openai-search`, `anthropic-search`, etc.) - API keys

**Authentication Optional:**
- `reddit` - Reddit API (Client ID/Secret recommended for higher rate limits)

### MCP Client Usage

#### 1. Initialize Connection

```json
{
  "jsonrpc": "2.0",
  "method": "initialize",
  "params": {
    "protocol_version": "0.1.0",
    "capabilities": {},
    "client_info": {
      "name": "your_client",
      "version": "1.0.0"
    }
  },
  "id": 1
}
```

#### 2. List Available Tools

```json
{
  "jsonrpc": "2.0",
  "method": "tools/list",
  "params": {},
  "id": 2
}
```

#### 3. Call a Tool

```json
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "wikipedia/search",
    "arguments": {
      "query": "rust programming language",
      "limit": 5
    }
  },
  "id": 3
}
```

#### 4. List Resources

```json
{
  "jsonrpc": "2.0",
  "method": "resources/list",
  "params": {},
  "id": 4
}
```

#### 5. Read Resource

```json
{
  "jsonrpc": "2.0",
  "method": "resources/read",
  "params": {
    "uri": "wikipedia://article/Rust_(programming_language)"
  },
  "id": 5
}
```

### Tool Naming Convention

Tools are prefixed with their connector name to avoid conflicts. Examples (not exhaustive):
- `hackernews/search` - Search Hacker News threads
- `hackernews/get_thread` - Fetch a compact Hacker News thread
- `wikipedia/search` - Search Wikipedia
- `wikipedia/get_article` - Get specific Wikipedia article
- `reddit/search_reddit` - Search Reddit posts
- `youtube/get_video_details` - Get YouTube video with transcript

Use `tools/list` to discover the full, exact tool set for each connector.

## Example Client

See `examples/mcp_client_example.rs` for a complete example of how to interact with the MCP server programmatically.

For LLM-oriented minimal guidance, see `docs/llms.txt` (task → tool mapping + tiny examples).

```bash
# Run the example client
cargo run --example mcp_client_example
```

## Integration with MCP Clients

### Claude Desktop

Add to your Claude Desktop configuration:

```json
{
  "mcpServers": {
    "rzn-tools": {
      "command": "/path/to/rzn-tools-mcp",
      "args": [],
      "env": {
        "GOOGLE_API_KEY": "your_api_key",
        "GOOGLE_CSE_ID": "your_cse_id"
      }
    }
  }
}
```

### Other MCP Clients

The server implements the standard MCP protocol and should work with any compliant client. Use stdio for local desktop MCP clients and HTTP mode when you want to front it with something like a Cloudflare Worker or tunnel.

## Server Architecture

```
┌─────────────────┐
│   MCP Client    │
└─────────┬───────┘
          │ JSON-RPC/stdio or HTTP
┌─────────▼───────┐
│  JsonRpcHandler │
└─────────┬───────┘
          │
┌─────────▼───────┐
│   McpServer     │
└─────────┬───────┘
          │
┌─────────▼───────┐
│ProviderRegistry │
└─────────┬───────┘
          │
┌─────────▼───────┐
│   Connectors    │
│ (hackernews,    │
│  wikipedia,     │
│  google, etc.)  │
└─────────────────┘
```

## Error Handling

The server properly handles and reports errors according to the JSON-RPC specification:

- Parse errors (-32700)
- Invalid params (-32602)
- Method not found (-32601)
- Internal errors (-32603)
- Connector-specific errors (mapped to appropriate codes)

## Logging

Set log level via environment variable:

```bash
export RUST_LOG=rzn_tools_mcp=debug
cargo run -p rzn_tools_mcp --bin rzn-tools-mcp
```

## Development

### Adding New Connectors

1. Implement the `Connector` trait in `src/connectors/`
2. Register the connector in `rzn_tools_core` so it is visible to `rzn-tools-mcp`
3. The connector will automatically be exposed via MCP

### Testing

```bash
# Test the library
cargo test

# Test MCP server
cargo run --example mcp_client_example
```

## Standards Compliance

This implementation follows the [Model Context Protocol specification](https://modelcontextprotocol.io/) and is compatible with MCP clients and tools.
