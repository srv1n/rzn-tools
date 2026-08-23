# rzn-tools MCP

This crate provides MCP over standard input and HTTP. Its default feature set contains no connectors.

```bash
make run CARGO_ARGS="-p rzn_tools_mcp --features server-full --"
make run CARGO_ARGS="-p rzn_tools_mcp --features server-full -- http --bind 127.0.0.1:8000"
```

The HTTP routes are `/`, `/mcp`, `/healthz`, and `/readyz`.

The server reads command arguments and environment variables. It does not read a TOML config file.

See the [MCP guide](../docs/system/mcp.md) and [Security guide](../docs/system/security.md).
