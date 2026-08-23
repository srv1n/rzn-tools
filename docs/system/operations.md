---
title: "Operations"
subject: operations
keywords: [install, assets, release, plugin]
part_of: overview
describes: [Makefile, packaging, resources, scripts/release.py, scripts/publish_rzn_tools_release.py]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to install, update, remove, or release rzn-tools."
skip_when: "You need source change rules."
---

# Operations

Install the binary and its assets together.

## Install a release

```bash
curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash
rzn-tools --version
rzn-tools workflows list
```

The installer supports macOS and Linux on Intel and ARM. It installs the binary and a workflow asset bundle.

The default paths are `~/.local/bin` and `~/.local/share/rzn-tools`. Set `INSTALL_DIR` and `ASSET_DIR` to change them.

## Install from this checkout

```bash
make install FEATURES=server-full
```

The local installer can also use `BUILD_EXAMPLES`, `INSTALL_DIR`, and `ASSET_DIR`.

## Asset lookup

The runtime checks asset roots in this order:

1. `RZN_TOOLS_ASSET_DIR`
2. the managed data directory under `rzn-tools/assets`
3. `share/rzn-tools` beside the installed binary prefix
4. the repository root

`rzn-tools workflows list` reports the active roots. `workflows sync` replaces the managed asset root through a staging directory.

## Inspect local state

```bash
rzn-tools config show
rzn-tools workflows list
rzn-tools usage --last 20
rzn-tools ingest sources
```

Auth, serve, ingest, and asset paths depend on the operating system. Usage events use `~/.rzn-tools/usage.jsonl`.

Before removal, inspect `which rzn-tools` and the active asset roots. A config-directory removal also removes credentials and ingest state.

## CLI release

The GitHub workflow builds CLI archives for Linux x64, Windows x64, macOS Intel, and macOS Apple Silicon. It also builds the workflow bundle and checksums.

```bash
make release-dry-run VERSION=<version>
make release VERSION=<version>
```

The release script requires a clean `main` branch that matches `origin/main`. It creates and pushes the tag.

## Plugin release

A plugin ZIP is only an artifact. A complete release also registers, uploads, publishes, and verifies the plugin.

Publish local first. Publish cloud second.

```bash
python3 scripts/publish_rzn_tools_release.py \
  --platform macos_arm64 \
  --channel stable \
  --targets all
```

The local backend is `http://localhost:8082`. The cloud backend is `https://cloud.rzn.ai`.

Stop when one target fails. Follow [`plugin_team_release_guide.md`](../../../backend/docs/runbook/plugin_team_release_guide.md) for the backend contract.
