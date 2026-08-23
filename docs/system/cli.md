---
title: "CLI"
subject: cli
keywords: [command line, commands, ingest, output]
part_of: overview
describes: [rzn_tools_cli/src/cli.rs, rzn_tools_cli/src/main.rs, rzn_tools_cli/src/commands]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to use or change the command line."
skip_when: "You need MCP transport details."
---

# CLI

Use the command line to list sources, read data, and run local tools.

The binary name is `rzn-tools`. [`cli.rs`](../../rzn_tools_cli/src/cli.rs) defines its command
grammar.

## Commands

| Purpose | Commands |
| --- | --- |
| Discover | `list`, `connectors`, `tools` |
| Configure | `setup`, `config` |
| Read | `search`, `get`, `fetch`, `formats`, `call` |
| Local pipeline | `ingest`, `pricing`, `usage`, `report` |
| Assets | `workflows`, `skills` |
| MCP service | `serve`, `configure` when the `serve` feature is on |
| Typed provider command | `youtube` |

Use help as the live command reference:

```bash
rzn-tools --help
rzn-tools search --help
rzn-tools tools youtube --output json
rzn-tools call --help
rzn-tools youtube --help
```

Only YouTube has a typed provider command. Use `tools` and `call` for other connectors.

## Basic use

```bash
rzn-tools list
rzn-tools setup youtube
rzn-tools config test youtube
rzn-tools search arxiv "retrieval systems"
rzn-tools fetch 'https://www.youtube.com/watch?v=dQw4w9WgXcQ'
rzn-tools call weather get_weather --args '{"location":"Pune"}'
```

`search <connector> <query>` uses one connector. Select `--profile` or `--sources` for federated search.

`get` and `search` support the mappings in [`tool_mappings.rs`](../../rzn_tools_cli/src/commands/tool_mappings.rs). Use `call` when no mapping exists.

`setup` asks for connector config fields and saves them. Run `config test` as a separate step.

## Output

Global `--output` values are `pretty`, `json`, `yaml`, `text`, and `markdown`.

`--copy` works on search, get, fetch, and connector output paths. It is not universal.

`fetch --output-format` asks for `raw`, `normalized_v1`, or `display_v1`. The tool can reject an unsupported value.

`get --field <name>` selects one top-level field.

## Auth profiles

`--auth-profile <name>` selects a named CLI profile. Some connectors read their default store key directly.

Use `rzn-tools config show` to inspect the active config file.

## Ingest

```bash
rzn-tools ingest sources
rzn-tools ingest add --help
rzn-tools ingest list
rzn-tools ingest remove --help
rzn-tools ingest run --help
```

An ingest run stores items, blocks, cursors, and seen IDs. It can run once or on an interval.

## Assets and usage

- `workflows list` shows bundled, managed, and active asset roots.
- `workflows sync` uses bundled assets unless `--remote` is present.
- `skills` manages the bundled agent skill.
- `usage` reads `~/.rzn-tools/usage.jsonl`.
- `pricing` reads embedded pricing data.
- `report tool-broken` prints a failure report draft.

Set `RZN_TOOLS_RUN_ID` to group usage events for one run.
