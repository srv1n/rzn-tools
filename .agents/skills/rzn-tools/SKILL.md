---
name: rzn-tools
description: Build, modify, package, install, or operate the RZN Integrations rzn-tools repo and CLI. Use when an agent needs to add/fix connectors, CLI commands, MCP server/runtime behavior, normalized_v1 ingestion output, auth setup, smart resolver/federated search, workflow bundles, the rzn-tools Agent Skill installer, or the rzn-tools plugin bundle/release flow. Use when the user asks how to run or validate rzn-tools locally. Do not use for general Rust questions, unrelated MCP servers, arbitrary Agent Skill creation, or ordinary research/data-fetching tasks unless the task specifically involves rzn-tools itself.
---

# RZN Tools

## Use And Skip

Use this skill for work on `rzn-tools` itself:

- build, test, release, or package the CLI/MCP runtime;
- add or repair connectors and connector tool schemas;
- update the `skills` installer command or bundled `rzn-tools` Agent Skill;
- wire feature flags through core, CLI, and MCP crates;
- implement `normalized_v1`, smart resolver, federated search, or ingest behavior;
- explain how to run `rzn-tools` commands from a local checkout or installed binary.

Skip this skill for:

- generic Rust help not tied to this repo;
- creating unrelated skills for other projects;
- using a connector merely to answer a research question;
- MCP design that does not touch `rzn-tools`;
- plugin release advice for non-`rzn-tools` bundles.

Treat `rzn-tools` as a local-first Rust integration runtime with three public surfaces:

```text
external systems
      |
rzn_tools_core  -> connector trait, registry, auth, resolver, normalized output
      |
  +---+----------------+
  |                    |
rzn_tools_cli      rzn_tools_mcp
CLI UX             MCP server/plugin runtime
```

## Working Rules

- Prefer existing connector, CLI, and MCP patterns over new abstractions.
- Keep changes feature-gated. Connector features must be forwarded through `rzn_tools_cli` and `rzn_tools_mcp`.
- Do not access personal-data connectors (`mail`, `notes`, `messages`, `reminders`, `contacts`) without explicit user permission.
- For user-facing behavior, update docs and `CHANGELOG.md` when the change is visible.
- Use shell/direct CLI commands for helper scripts. Do not add Python scripts to this skill.
- If plugin release work is requested, building a ZIP is not done. Backend register/publish notification is required.

## Core Workflow

1. Inspect the relevant repo docs before editing:
   - Connector work: `references/connector-development.md`
   - CLI/MCP behavior: `references/cli-mcp.md`
   - Normalized ingestion output: `references/normalized-output.md`
   - Plugin packaging/release: `references/plugin-release.md`
2. Read the current code around the target connector or command. This repo moves quickly; docs can lag.
3. Make the narrowest code change that preserves the shared model across core, CLI, and MCP.
4. Validate with the smallest useful command first, then broaden:
   - `cargo fmt --all`
   - `cargo check -p rzn_tools_cli --features "<feature>"`
   - `cargo test -p rzn_tools_core <test-filter>`
   - `cargo clippy --all-targets --all-features -- -D warnings`
   - `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`

Run `scripts/validate.sh` for a shell-only validation wrapper.

## Connector Checklist

For a new connector, complete this path unless the task is explicitly smaller:

```text
core module
  -> Connector impl
  -> feature flag
  -> registry registration
  -> CLI/MCP feature forwarding
  -> optional direct CLI subcommand
  -> resolver/federated search if useful
  -> normalized_v1 for indexable tools
  -> docs/tests
```

Minimum code touchpoints:

- `rzn_tools_core/src/connectors/<name>/mod.rs`
- `rzn_tools_core/src/connectors/mod.rs`
- `rzn_tools_core/src/lib.rs`
- `rzn_tools_core/Cargo.toml`
- `rzn_tools_cli/Cargo.toml`
- `rzn_tools_mcp/Cargo.toml`
- `README.md` and/or `docs/connectors/<name>.md`

Add a direct CLI subcommand in `rzn_tools_cli/src/cli.rs` and command dispatch only when the connector needs a polished user-facing CLI. Otherwise, the generic `tools`, `search`, `get`, and MCP surfaces may be enough.

## Output Defaults

Use these defaults unless the surrounding code proves otherwise:

| Use case | Output knob | Default |
|---|---:|---|
| LLM tool responses | `response_format` | `concise` |
| Full provider payloads | `response_format` | `detailed` |
| Existing connector shape | `output_format` | `raw` |
| Ingestion/indexing | `output_format` | `normalized_v1` |
| UI-friendly display data | `output_format` | `display_v1` when supported |

For indexable list/search/get tools, prefer `output_format: "raw" | "normalized_v1"` plus `limit` and opaque `cursor` for pageable outputs. Normalized output belongs in `CallToolResult.structured_content`, not duplicated into text content.

## Gotchas

- Release builds must use all features: `cargo build --release -p rzn_tools_cli --features full`.
- `rzn_tools_mcp` default features are intentionally empty; plugin/MCP release builds need `--features full`.
- Connector names use hyphens publicly (`google-drive`) and module names use underscores (`google_drive`).
- Register aliases when old names or common spellings exist (`semantic_scholar`, `gsc`, `x-cookies`, etc.).
- Personal-data connectors require explicit user permission before testing against real local data.
- Admin/watch/subscribe tools should stay hidden unless `RZN_SHOW_ADMIN_TOOLS=1`.
- If a normalized tool supports paging, `next_cursor` must be top-level and `has_more` must match cursor presence.
- Plugin release completion requires local backend publish first, then cloud publish. Stop on the first failure.

## Validation Commands

For a quick local sanity check:

```bash
.agents/skills/rzn-tools/scripts/validate.sh quick
```

For a broader pre-PR check:

```bash
.agents/skills/rzn-tools/scripts/validate.sh full
```

Use targeted feature checks during connector work:

```bash
cargo check -p rzn_tools_cli --features "youtube,hackernews"
cargo run -p rzn_tools_cli --features "youtube" -- youtube --help
cargo run -p rzn_tools_cli --features "hackernews" -- tools hackernews --output json
```

## Release Boundary

For ordinary CLI/library releases, follow repo release docs and existing targets.

For plugin bundle release work, load `references/plugin-release.md` before claiming completion. A signed ZIP alone is a half-release; the backend catalog must be registered, published, and verified in local then cloud environments.
