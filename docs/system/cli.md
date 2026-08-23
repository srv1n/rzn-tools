---
subject: cli
keywords: [command line, commands, usage, ingest]
part_of: overview
describes: [rzn_tools_cli/src/cli.rs, rzn_tools_cli/src/main.rs, rzn_tools_cli/src/commands]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to use or change the command line interface."
skip_when: "You need MCP transport details. Open mcp.md."
---

# CLI

Use the command-line program to find sources, read data, manage local state,
and start the server.

The binary is `rzn-tools`. The command definition is in
`rzn_tools_cli/src/cli.rs`. The dispatch code is in
`rzn_tools_cli/src/main.rs`.

## Command groups

| Group | Commands |
| --- | --- |
| Discover and configure | `list` (`ls`), `setup` (`init`), `config`, `connectors`, `tools` |
| Read data | `search`, `get`, `fetch` (`f`), `formats` (`patterns`), `call` |
| Local pipelines | `ingest`, `pricing`, `usage`, `report tool-broken` |
| Assets and agents | `workflows` (`systems`), `skills` |
| MCP service | `serve`, `configure cloudflare` when the `serve` feature is built |
| Provider wrappers | The connector commands shown by `rzn-tools --help` |

Use help as the live command catalog:

```bash
rzn-tools --help
rzn-tools search --help
rzn-tools youtube --help
rzn-tools tools youtube --output json
```

Provider wrappers give stronger argument checks. `call` is the generic path
for tools that do not have a wrapper.

## Common use

```bash
rzn-tools list
rzn-tools setup github
rzn-tools search arxiv "retrieval systems"
rzn-tools search "retrieval systems" --profile research
rzn-tools get github owner/repository
rzn-tools fetch 'https://www.youtube.com/watch?v=dQw4w9WgXcQ'
rzn-tools call weather get_weather --args '{"location":"Pune"}'
```

The default federated search profile is `research` when no source or profile is
given. Built-in profiles include `research`, `enterprise`, `social`, `code`,
`web`, and `media`. Use `--sources`, `--add`, and `--exclude` to change one
request. Use `--merge grouped` or `--merge interleaved` to select the merge.

## Output

Global `--output` values are `pretty`, `json`, `yaml`, `text`, and `markdown`.
`--copy` copies the final output. `fetch --output-format` is different: it asks
a connector for `raw`, `normalized_v1`, or `display_v1` only when that tool
supports the field.

`get --field <name>` extracts one top-level field. JSON and YAML output from
`connectors` contain connector names, but the detailed status and capability
view is currently available only in pretty output.

`--tui` is used when the TUI feature is present. `--no-color` and `--verbose`
are accepted, but the current command path does not apply them consistently.

## Auth profiles

`--auth-profile <name>` selects a stored CLI profile. Profile keys use
`provider::profile`; the plain provider key is the default profile.

```bash
rzn-tools --auth-profile work setup github
rzn-tools --auth-profile work config test github
```

The file path is platform-specific. Run `rzn-tools config show` instead of
assuming a fixed home-directory path.

## Ingest

`ingest` manages local source definitions and writes normalized JSONL data.

```bash
rzn-tools ingest sources list
rzn-tools ingest sources add --help
rzn-tools ingest run --help
```

The source config and index files are below the platform config directory. A
named tenant gets its own directory. The run command stores items, blocks,
cursors, seen IDs, and errors. `--interval-seconds` runs until it is stopped.
This command writes local data; review the source and tenant before you run it.

## Usage, pricing, workflows, and skills

- `usage` reads local usage events and can filter by time or run ID.
- `pricing` reads the embedded pricing catalog.
- `report tool-broken` prints a cleaned failure report draft.
- `workflows list` shows bundled, managed, and active assets.
- `workflows sync` downloads a released workflow bundle.
- `skills` installs or links the bundled agent skill for a selected client.

`RZN_TOOLS_RUN_ID` sets the usage run ID for a command.

## Input rules

Quote shell URLs that contain `?`. Use a prefix when an ID is ambiguous:

```bash
rzn-tools fetch hn:12345678
rzn-tools fetch PMID:12345678
rzn-tools fetch arXiv:2301.07041
```
