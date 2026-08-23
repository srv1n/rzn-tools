# rzn-tools

`rzn-tools` gives one interface to many web, cloud, mail, market, and local data sources.

The project has three crates:

- `rzn_tools_core` contains connectors and shared behavior.
- `rzn_tools_cli` provides the `rzn-tools` command.
- `rzn_tools_mcp` provides MCP over standard input and HTTP.

## Start

Install `sccache` once:

```bash
cargo install sccache --locked
```

Build and inspect the default CLI:

```bash
make build CARGO_ARGS="-p rzn_tools_cli"
make run CARGO_ARGS="-p rzn_tools_cli -- list"
make run CARGO_ARGS="-p rzn_tools_cli -- tools youtube"
```

After installation, common calls are:

```bash
rzn-tools search arxiv "retrieval systems"
rzn-tools call weather get_weather --args '{"location":"Pune"}'
rzn-tools fetch 'https://www.youtube.com/watch?v=dQw4w9WgXcQ'
```

Use shell quoting that preserves the JSON and URL text.

## Build sets

The default CLI contains the `default-connectors` set. Use `server-full` for the portable server set.

```bash
make build-release CARGO_ARGS="-p rzn_tools_cli --features server-full"
make build-release CARGO_ARGS="-p rzn_tools_mcp --features server-full"
```

`all-connectors` adds Telegram and macOS connectors. `desktop-full` also adds browser-profile import and `x-browser`. Discord stays opt-in.

## MCP

Standard input:

```bash
make run CARGO_ARGS="-p rzn_tools_mcp --features server-full --"
```

HTTP:

```bash
make run CARGO_ARGS="-p rzn_tools_mcp --features server-full -- http --bind 127.0.0.1:8000"
```

The HTTP server has no built-in user authentication or TLS. Keep it local or use an authenticated proxy.

## Output

Global `--output` controls CLI rendering. Some tools also accept `output_format`.

The live tool values are `raw`, `normalized_v1`, and `display_v1`. The `_v1` text is part of the current wire value.

Check a tool before use:

```bash
rzn-tools tools <connector> --output json
rzn-tools call <connector> <tool> --args '<json-object>'
```

## Documentation

[`docs/system/`](docs/system/) is the only product documentation corpus.

- [System overview](docs/system/00-overview.md)
- [Architecture](docs/system/architecture.md)
- [CLI](docs/system/cli.md)
- [Connectors](docs/system/connectors.md)
- [MCP](docs/system/mcp.md)
- [Development](docs/system/development.md)
- [Operations](docs/system/operations.md)
- [Security](docs/system/security.md)

The source code and live tool schema are the final authority.

## License

GNU Affero General Public License 3.0 only. See [LICENSE](LICENSE).
