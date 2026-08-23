# Packaging

The release contains two artifact groups:

| Artifact | Content |
| --- | --- |
| `rzn-tools-<tag>-<target>.tar.gz` or `.zip` | CLI binary. |
| `rzn-tools-workflows-<tag>.tar.gz` | Systems, icons, and examples. |

The shell installer needs both groups.

## Local install

```bash
make install FEATURES=server-full
```

Set `INSTALL_DIR` and `ASSET_DIR` to change the destination.

## Release

```bash
make release-dry-run VERSION=<version>
make release VERSION=<version>
```

The release script checks the tree, branch, and remote state. GitHub Actions builds the platform archives, workflow bundle, and checksums.

See [Operations](../docs/system/operations.md) for the complete release and plugin publish flow.
