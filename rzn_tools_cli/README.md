# rzn-tools CLI

The CLI is the terminal front end for the connectors in `rzn_tools_core`.

```bash
make build CARGO_ARGS="-p rzn_tools_cli"
make run CARGO_ARGS="-p rzn_tools_cli -- list"
make run CARGO_ARGS="-p rzn_tools_cli -- --help"
```

Use `rzn-tools <command> --help` for exact flags. See:

- [CLI guide](../docs/system/cli.md)
- [Connector catalog](../docs/system/connectors.md)
- [Development](../docs/system/development.md)
- [Operations](../docs/system/operations.md)

The workspace baseline is Rust 1.75. Some optional full or desktop features
need a newer compiler because of patched dependencies.
