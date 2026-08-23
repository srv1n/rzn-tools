# Architecture

The current architecture guide is [system/architecture.md](system/architecture.md).
It describes the connector trait, registry, resolver, feature profiles, output
contracts, local state, and code paths.

Use the source as the final authority:

- `rzn_tools_core/src/lib.rs` for connectors and the registry
- `rzn_tools_core/src/resolver.rs` for URL and ID routing
- `rzn_tools_cli/src/` for CLI behavior
- `rzn_tools_mcp/src/` for MCP transports
