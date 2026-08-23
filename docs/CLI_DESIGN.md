# CLI design

The implemented CLI contract is in [system/cli.md](system/cli.md).

Use these commands for the live surface:

```bash
rzn-tools --help
rzn-tools <command> --help
rzn-tools tools [connector] --output json
```

The command types are in `rzn_tools_cli/src/cli.rs`. Dispatch is in
`rzn_tools_cli/src/main.rs`. Old design examples are not a runtime contract.
