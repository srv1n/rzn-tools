# rzn-tools MCP server

The MCP server exposes feature-enabled connectors over stdio or HTTP. The
crate's default feature set has no connectors.

```bash
# stdio
make run CARGO_ARGS="-p rzn_tools_mcp --features full --"

# HTTP
make run CARGO_ARGS="-p rzn_tools_mcp --features full -- http --bind 127.0.0.1:8000"
```

HTTP routes are `/`, `/mcp`, `/healthz`, and `/readyz`. After an
initialize request, send the returned `mcp-session-id` with each request.
GET SSE is not implemented. A POST can return one SSE event.

The binary uses flags and environment values. It does not read
`mcp_config.example.toml`.

See:

- [MCP guide](../docs/system/mcp.md)
- [Security](../docs/system/security.md)
- [Connector catalog](../docs/system/connectors.md)
