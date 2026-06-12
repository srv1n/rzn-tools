# Packaging and Distribution

This repo now ships two release artifact types:

| Artifact | Purpose |
|---|---|
| `rzn-tools-<tag>-<target>.tar.gz` / `.zip` | CLI binary for a specific platform |
| `rzn-tools-workflows-<tag>.tar.gz` | bundled starter workflow/example/icon assets (`resources/{systems,icons}` + `examples`) |

That split is intentional. The shell installer and the CLI workflow sync command both consume the
same workflow bundle.

## Local Install

```bash
make install
```

What it does:

1. builds `rzn-tools` in release mode
2. compiles the Rust example binaries
3. validates bundled workflow metadata
4. installs the binary plus bundled assets

Default install layout:

```text
~/.local/bin/rzn-tools
~/.local/share/rzn-tools/resources/systems/...
~/.local/share/rzn-tools/resources/icons/connectors/...
~/.local/share/rzn-tools/examples/...
```

Override locations if you need to:

```bash
INSTALL_DIR=/usr/local/bin ASSET_DIR=/usr/local/share/rzn-tools make install
```

## Shell Installer

```bash
curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash
```

The installer:

- detects the platform
- downloads the matching CLI archive
- downloads the matching workflow bundle
- installs both

## Release Builders

Docker/cross:

```bash
./packaging/scripts/build-all.sh
```

Zigbuild:

```bash
./packaging/scripts/build-all-zigbuild.sh
```

Both scripts:

- build release binaries using current `rzn_tools_*` package names
- validate example compilation + bundled workflow metadata before packaging
- emit the workflow bundle expected by `rzn-tools workflows sync --remote`

Output goes to:

```text
target/releases/
```

## GitHub Release Workflow

`.github/workflows/release.yml` publishes:

- Linux CLI archive
- macOS CLI archives
- Windows CLI archive
- workflow bundle archive
- `checksums.txt`

Release notes also point users at:

```bash
rzn-tools workflows list
rzn-tools workflows sync --remote
```

## Cutting A Release

Use the repo entrypoint, not ad-hoc `git tag` muscle memory:

```bash
make release VERSION=0.2.17
```

What it does:

1. verifies the tree is clean
2. verifies you are releasing from `main`
3. verifies local `main` exactly matches `origin/main`
4. creates and pushes the annotated tag
5. lets GitHub Actions build Linux, Windows, macOS Intel, macOS Apple Silicon, workflow assets, checksums, and publish the GitHub Release

Preview it without touching git:

```bash
make release-dry-run VERSION=0.2.17 ALLOW_DIRTY=1 SKIP_REMOTE_CHECK=1
```

Normalize historical GitHub release titles that still use the legacy brand:

```bash
GITHUB_TOKEN=... make release-retitle-legacy
```

## Manual Artifact Install

If you are installing from raw artifacts instead of the shell script, you need both:

1. the platform binary archive
2. `rzn-tools-workflows-<tag>.tar.gz`

If you only install the binary, you have a partial install.
