# Installation

Install the CLI and its assets:

```bash
curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash
rzn-tools --version
rzn-tools workflows list
```

Install from this checkout:

```bash
make install FEATURES=server-full
```

The default install paths are `~/.local/bin` and `~/.local/share/rzn-tools`.

Set `INSTALL_DIR` and `ASSET_DIR` to change these paths. Set `RZN_TOOLS_ASSET_DIR` to select a runtime asset root.

See [Operations](docs/system/operations.md) for asset lookup, removal, and release commands.
