# RZN Integrations Installation (`rzn-tools`)

The `rzn-tools` install for **RZN Integrations** now has two pieces:

- the CLI binary
- the bundled workflow/example assets under `resources/systems` and `examples`

That second part matters. The install is not complete if the binary exists but the starter workflows do not.

## Quick Install

macOS and Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash
```

That installer downloads:

- the correct release binary for your platform
- `rzn-tools-workflows-<tag>.tar.gz`

Then it installs:

- binary: `~/.local/bin/rzn-tools` by default
- assets: `~/.local/share/rzn-tools` by default

Override paths if you want:

```bash
INSTALL_DIR=/usr/local/bin ASSET_DIR=/usr/local/share/rzn-tools \
  curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash
```

## Verify The Install

```bash
rzn-tools --version
rzn-tools workflows list
rzn-tools setup
```

You should see an active workflow asset root and the bundled starter systems.

## Local Source Install

If you are working from the repo, this is the path that matters:

```bash
make install
```

`make install` now does three things:

1. builds the release CLI
2. compiles the Rust example binaries and validates bundled workflow metadata
3. installs the binary plus bundled workflow/example assets locally

By default it installs to:

- binary: `~/.local/bin`
- assets: `~/.local/share/rzn-tools`

Optional overrides:

```bash
INSTALL_DIR=/usr/local/bin make install
ASSET_DIR=/usr/local/share/rzn-tools make install
BUILD_EXAMPLES=0 make install
FEATURES="youtube,hackernews" make install
```

`BUILD_EXAMPLES=0` skips the example build/validation pass. Leave it on unless you are in a hurry.

## Workflow Assets

Starter workflows/examples are real assets now, not repo trivia.

Inspect what is currently active:

```bash
rzn-tools workflows list
```

Sync the bundled copy from your current install into the managed user directory:

```bash
rzn-tools workflows sync
```

Pull the latest published workflow bundle from GitHub Releases:

```bash
rzn-tools workflows sync --remote
```

Pull a specific release:

```bash
rzn-tools workflows sync --remote --version v0.2.17
```

The managed sync location is:

- macOS/Linux: `~/.local/share/rzn-tools/assets`

Asset resolution order is:

1. `RZN_TOOLS_ASSET_DIR`
2. managed synced assets (`~/.local/share/rzn-tools/assets`)
3. install prefix share dir (`.../share/rzn-tools`)
4. repo root when running from source

## Manual Release Artifact Install

If you do not want the shell installer, download two assets from the release:

- `rzn-tools-<tag>-<target>.tar.gz`
- `rzn-tools-workflows-<tag>.tar.gz`

Example for Apple Silicon macOS:

```bash
TAG=v0.2.17
curl -LO "https://github.com/srv1n/rzn-tools/releases/download/${TAG}/rzn-tools-${TAG}-aarch64-apple-darwin.tar.gz"
curl -LO "https://github.com/srv1n/rzn-tools/releases/download/${TAG}/rzn-tools-workflows-${TAG}.tar.gz"

tar -xzf "rzn-tools-${TAG}-aarch64-apple-darwin.tar.gz"
tar -xzf "rzn-tools-workflows-${TAG}.tar.gz"

mkdir -p ~/.local/bin ~/.local/share/rzn-tools
cp rzn-tools ~/.local/bin/
cp -R share/rzn-tools/. ~/.local/share/rzn-tools/
```

For Linux, swap the target triple:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Windows release assets are published too, but the one-line shell installer is intentionally for macOS/Linux.

## Build From Source Manually

If you want the raw commands instead of `make install`:

```bash
cargo build --release -p rzn_tools_cli --features full
cargo build --release -p rzn_tools_core --features "examples,full" --examples
cargo test -p rzn_tools_core --test system_metadata_conformance

mkdir -p ~/.local/bin ~/.local/share/rzn-tools
cp target/release/rzn-tools ~/.local/bin/
cp -R resources ~/.local/share/rzn-tools/
cp -R examples ~/.local/share/rzn-tools/
```

## Updating

Update the binary and bundled assets:

```bash
curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash
```

Only refresh workflows/examples:

```bash
rzn-tools workflows sync --remote
```

## Uninstall

```bash
rm -f ~/.local/bin/rzn-tools
rm -rf ~/.local/share/rzn-tools
rm -rf ~/.config/rzn-tools
```

If you installed system-wide, remove the matching `/usr/local/bin/rzn-tools` and `/usr/local/share/rzn-tools`.

## Troubleshooting

Command not found:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

Missing workflow assets:

```bash
rzn-tools workflows list
rzn-tools workflows sync
```

Want the latest published workflows:

```bash
rzn-tools workflows sync --remote
```

macOS Gatekeeper being annoying:

```bash
xattr -d com.apple.quarantine ~/.local/bin/rzn-tools
```
