---
subject: development
keywords: [build, test, rust, contribution, validation]
part_of: overview
describes: [Cargo.toml, Makefile, rzn_tools_core, rzn_tools_cli, rzn_tools_mcp, vendor]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to build, test, or change this repository."
skip_when: "You only need to install a released binary. Open operations.md."
---

# Development

Use the repository commands to build and check changes. Start with the
smallest check that covers the files you changed.

## Build rule

Use a Make target for every Rust compile, run, lint, test, or documentation
command. Do not call Cargo directly. The Make targets use `sccache`.

```bash
make build CARGO_ARGS="-p rzn_tools_cli"
make build-release CARGO_ARGS="-p rzn_tools_cli --features full"
make run CARGO_ARGS="-p rzn_tools_cli -- list"
make test CARGO_ARGS="--workspace"
make fmt
make fmt-check
make clippy CARGO_ARGS="--all-targets --all-features -- -D warnings"
RUSTDOCFLAGS="-D warnings" make doc CARGO_ARGS="--workspace"
```

`make check-server-profile` checks the portable MCP dependency profile. Run
the smallest useful check first. Run the full checks before a release.

## Source map

- `rzn_tools_core/src/lib.rs`: connector trait, registry, and feature registration
- `rzn_tools_core/src/connectors/`: provider adapters
- `rzn_tools_core/src/resolver.rs`: URL and ID resolver rules
- `rzn_tools_core/src/ingest.rs`: normalized output types
- `rzn_tools_core/src/display/`: display output types and conversion
- `rzn_tools_core/src/federated/`: search profiles and merge engine
- `rzn_tools_cli/src/cli.rs`: CLI shape
- `rzn_tools_cli/src/commands/`: CLI behavior
- `rzn_tools_mcp/src/`: MCP transport and server startup
- `resources/` and `examples/`: installed system assets
- `vendor/`: patched dependencies

## Rust versions

The workspace baseline is Rust 1.75 and edition 2021. Some optional patched
dependencies need a newer compiler:

- the full YouTube profile includes a crate with Rust 1.79 metadata
- Telegram uses an edition-2024 patched crate and needs a compiler that
  supports edition 2024

Treat 1.75 as the minimum for the base workspace, not every feature profile.
Formatting uses a 100-character line width.

## Tests and proof

Mock provider calls. Do not use personal-data connectors in automated tests.
The main contract checks include:

- normalized fixtures and tool-schema conformance
- resolver routes
- launcher metadata and quickstart parity
- system metadata and icon manifest parity

A focused green test proves only that path. It does not prove a live provider,
a packaged release, a public deployment, or human acceptance.

## Documentation

`docs/system/` is the canonical system guide. Provider pages can add details,
but they must not contradict the system guide or the live tool schema. Use
short sentences. Define uncommon terms. State limits and side effects.

Run the Tusker documentation validation after a canonical page changes.
