# rzn-tools CLI

This crate provides the `rzn-tools` command.

```bash
make build CARGO_ARGS="-p rzn_tools_cli"
make run CARGO_ARGS="-p rzn_tools_cli -- --help"
make run CARGO_ARGS="-p rzn_tools_cli -- list"
```

Use `rzn-tools tools <connector>` for the live tool schema. Use `rzn-tools call` for connectors without a typed CLI command.

See the [CLI guide](../docs/system/cli.md).
