---
subject: operations
keywords: [install, release, assets, run, plugin]
part_of: overview
describes: [INSTALLATION.md, Makefile, packaging, resources, scripts/publish_rzn_tools_release.py]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You need to install, update, remove, or release rzn-tools."
skip_when: "You need source changes or tests. Open development.md."
---

# Operations

Install the command and its data files together. Check the active paths before
you update or remove an installation.

## Install a release

```bash
curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash
rzn-tools --version
rzn-tools workflows list
```

The shell installer supports macOS and Linux on Intel and ARM. It downloads a
binary archive and a workflow bundle. The default installer paths are
`~/.local/bin` and `~/.local/share/rzn-tools`. Use `INSTALL_DIR` and
`ASSET_DIR` to change them.

The runtime also has platform-managed config and data directories. On macOS
these are normally below `~/Library/Application Support`. On Linux they are
normally below `~/.config` and `~/.local/share`. Do not use one hard-coded
path for every platform.

## Install from this repository

```bash
make install
```

`FEATURES`, `BUILD_EXAMPLES`, `INSTALL_DIR`, and `ASSET_DIR` change the
local install.

## Assets

The runtime checks these roots in order:

1. `RZN_TOOLS_ASSET_DIR`
2. the platform-managed asset directory
3. the executable-prefix share directory
4. the repository root

`rzn-tools workflows list` shows the active, bundled, and managed roots.
`rzn-tools workflows sync` installs bundled assets or a GitHub release bundle
through a staging directory.

Each bundled system has `system.metadata.yaml` and matching quickstarts under
`examples/`. Tests check metadata, quickstarts, tool references, and icons.

## Configuration and local state

Use commands to inspect state:

```bash
rzn-tools config show
rzn-tools workflows list
rzn-tools usage --last
rzn-tools ingest sources list
```

Auth, serve, ingest, and managed asset paths are platform-specific. Usage
events are stored separately at `~/.rzn-tools/usage.jsonl`.

## Remove an install

Remove only paths that you confirmed with `which rzn-tools`,
`rzn-tools workflows list`, and `rzn-tools config show`. Removing the config
directory also removes auth profiles and ingest state. Back it up first if you
need those files.

## CLI release

The official release workflow builds:

- Linux x64
- Windows x64 with MSVC
- macOS Intel
- macOS Apple Silicon
- the workflow asset bundle
- checksums

Maintainers use:

```bash
make release-dry-run VERSION=<version>
make release VERSION=<version>
```

The release command requires a clean `main` branch that matches
`origin/main`. It creates and pushes the version tag. GitHub Actions builds
the release. The legacy `packaging/scripts/build-all*.sh` scripts have a
different target matrix and are not the official release definition.

## Plugin release

A plugin ZIP is not a completed release. The plugin version in
`scripts/plugins/config/rzn-tools.json` must match the Cargo release.

The required publish order is:

1. Build the plugin bundle.
2. Register and publish to `http://localhost:8082`.
3. Verify its catalog entry and artifact.
4. Register and publish to `https://cloud.rzn.ai`.
5. Verify its catalog entry and artifact.

```bash
python3 scripts/publish_rzn_tools_release.py \
  --platform macos_arm64 \
  --channel stable \
  --targets all
```

Stop on the first failed target. See
`../backend/docs/runbook/plugin_team_release_guide.md` for the backend
contract.
