# rzn-tools MCP

This crate provides MCP over standard input and HTTP. Its default feature set contains no connectors.

```bash
make run CARGO_ARGS="-p rzn_tools_mcp --features server-full --"
make run CARGO_ARGS="-p rzn_tools_mcp --features server-full -- http --bind 127.0.0.1:8000"
```

The HTTP routes are `/`, `/mcp`, `/healthz`, and `/readyz`.

The server reads command arguments and environment variables. It does not read a TOML config file.

For a backend-launched tenant sidecar, use the invocation-scoped profile:

```bash
rzn-tools-mcp sidecar --connectors youtube,hackernews
```

Sidecar mode refuses inherited credential environment variables, keeps `secrets/set` credentials in
process memory, and exits successfully on stdin EOF. `make sidecar-package` creates the
digest-addressed artifact; verify it with `make sidecar-verify SIDECAR_DIR=target/sidecars/<sha256>`.

See the [MCP guide](../docs/system/mcp.md) and [Security guide](../docs/system/security.md).
