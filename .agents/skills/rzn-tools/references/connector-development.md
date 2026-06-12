# Connector Development Reference

Use this when adding or changing an integration connector.

## Implementation Map

| Concern | Location |
|---|---|
| Connector trait, registry, aliases | `rzn_tools_core/src/lib.rs` |
| Module declarations | `rzn_tools_core/src/connectors/mod.rs` |
| Connector implementation | `rzn_tools_core/src/connectors/<name>/mod.rs` |
| Smart URL/ID routing | `rzn_tools_core/src/resolver.rs` and `Connector::url_patterns()` |
| Federated profiles | `rzn_tools_core/src/federated/profiles.rs` |
| CLI feature forwarding | `rzn_tools_cli/Cargo.toml` |
| MCP feature forwarding | `rzn_tools_mcp/Cargo.toml` |
| Direct CLI command definitions | `rzn_tools_cli/src/cli.rs` |
| Connector docs | `docs/connectors/<name>.md` |

## Connector Trait Expectations

Implement `Connector` from `rzn_tools_core/src/lib.rs`:

- `name()`: public connector id, usually hyphenated.
- `description()`: short human/tooling description.
- `display_name()`, `icon()`, `categories()`: UI/discovery metadata where useful.
- `requires_auth()`: true for credential-gated connectors.
- `credential_provider()`: override when credentials are shared with another provider.
- `url_patterns()`: smart resolver support for URLs/IDs.
- `list_tools()` and `call_tool()`: MCP tool surface.
- `config_schema()`, `get_auth_details()`, `set_auth_details()`, `test_auth()`: auth setup.

Prefer typed argument structs with `serde::Deserialize` for non-trivial tools. Keep schemas and parsing in sync.

## Feature Wiring

For a connector named `my-service` with module `my_service`:

1. Add a feature in `rzn_tools_core/Cargo.toml`.
2. Add `#[cfg(feature = "my-service")] pub mod my_service;` in `rzn_tools_core/src/connectors/mod.rs`.
3. Register it in `build_registry_enabled_only()` in `rzn_tools_core/src/lib.rs`.
4. Forward the feature in `rzn_tools_cli/Cargo.toml`.
5. Forward the feature in `rzn_tools_mcp/Cargo.toml`.
6. Add it to `all-connectors` or a narrower default set only when it belongs there.

Release builds use:

```bash
cargo build --release -p rzn_tools_cli --features full
```

## Tool Design

Use stable, boring tool names:

| Operation | Preferred shape |
|---|---|
| Search | `search`, `search_<domain>` |
| List/feed | `list`, `list_<items>` |
| Fetch one item | `get`, `get_<item>` |
| Auth helpers | `auth_start`, `auth_poll`, `test_auth` |
| Mutations | explicit verbs: `create_event`, `send_mail`, `submit_url` |

Indexable list/search/get tools should support:

- `limit` with clamping.
- `cursor` for pageable results.
- `output_format` with `raw` and `normalized_v1`.
- `response_format` with `concise` and `detailed` when LLM-facing payload size matters.

Mutation tools do not need normalized output unless there is a real ingestion use case.

## Auth Rules

- Never hardcode secrets.
- Use `ConnectorConfigSchema` so `rzn-tools setup <connector>` can guide users.
- Prefer OAuth/device-code helpers already present in the repo for Google/Microsoft-like connectors.
- For optional-auth connectors, work without credentials and improve with credentials.
- For personal-data connectors, provide commands for the user to run unless explicit permission was given to test locally.

## Smart Resolver

Use `Connector::url_patterns()` for connector-owned URL patterns. Add central resolver logic only when cross-connector disambiguation is needed.

Good resolver behavior:

- URLs and well-known IDs route without extra flags.
- Ambiguous numeric IDs require prefixes or interactive selection.
- Prefixes should be documented (`PMID:`, `hn:`, `arXiv:`).
- Any generic URL may fall back to the web scraper.

## Tests

Add tests at the level that catches the failure:

- Unit tests for parsing, schemas, cursors, item refs, and output shaping.
- Connector tests with mocked responses for API behavior.
- Conformance tests for normalized output.
- CLI checks for new user-facing command flags.

Avoid real network calls in CI.
