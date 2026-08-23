---
title: "Development"
subject: development
keywords: [build, test, rust, validation]
part_of: overview
describes: [Cargo.toml, Makefile, rzn_tools_core, rzn_tools_cli, rzn_tools_mcp]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to build, test, or change this repository."
skip_when: "You only need to install the command."
---

# Development

Use the project build rules for all development work.

Use a Make target for each Rust build, run, check, test, lint, format, and documentation command.
The Makefile configures `sccache`.

```bash
make build CARGO_ARGS="-p rzn_tools_cli"
make build-release CARGO_ARGS="-p rzn_tools_cli --features server-full"
make run CARGO_ARGS="-p rzn_tools_cli -- list"
make check CARGO_ARGS="--workspace"
make test CARGO_ARGS="--workspace"
make fmt
make fmt-check
make clippy CARGO_ARGS="--all-targets --all-features -- -D warnings"
RUSTDOCFLAGS="-D warnings" make doc CARGO_ARGS="--workspace"
```

Run the smallest check that covers your change. Run the complete checks before a release.

## Source map

| Path | Purpose |
| --- | --- |
| [`rzn_tools_core/src/lib.rs`](../../rzn_tools_core/src/lib.rs) | Connector trait and registry. |
| [`rzn_tools_core/src/connectors/`](../../rzn_tools_core/src/connectors/) | Provider and local-source adapters. |
| [`rzn_tools_core/src/resolver.rs`](../../rzn_tools_core/src/resolver.rs) | URL and ID routing. |
| [`rzn_tools_core/src/ingest.rs`](../../rzn_tools_core/src/ingest.rs) | Normalized records. |
| [`rzn_tools_core/src/display/`](../../rzn_tools_core/src/display/) | Display records and conversion. |
| [`rzn_tools_core/src/federated/`](../../rzn_tools_core/src/federated/) | Multi-source search. |
| [`rzn_tools_cli/src/cli.rs`](../../rzn_tools_cli/src/cli.rs) | CLI grammar. |
| [`rzn_tools_cli/src/commands/`](../../rzn_tools_cli/src/commands/) | CLI behavior. |
| [`rzn_tools_mcp/src/`](../../rzn_tools_mcp/src/) | MCP startup and HTTP transport. |
| [`resources/`](../../resources/) | Installed system assets. |
| [`examples/`](../../examples/) | Quickstarts and examples. |

## Change rules

- Mock remote provider calls in tests.
- Do not use personal data in automated tests.
- Keep connector calls inside the connector module.
- Keep shared behavior in the core library.
- Add a typed CLI command only when it gives useful input checks.
- Use one public name for each connector and tool.
- Update the canonical page when public behavior changes.

## Proof

A focused test proves only its tested path. It does not prove a live provider, an installed package, a release, or user acceptance.

## Documentation

`docs/system/` is the only product documentation corpus. Use short sentences. Use one term for one thing. State side effects and limits.

After a documentation change, run:

```bash
tusker docs map --json
tusker validate --json
```
