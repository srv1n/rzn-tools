#!/usr/bin/env bash
set -euo pipefail

# Build and install the local rzn-tools CLI binary.
# Usage:
#   ./packaging/scripts/local-install.sh
#   INSTALL_DIR=/usr/local/bin ./packaging/scripts/local-install.sh
#   FEATURES="youtube,hackernews" ./packaging/scripts/local-install.sh

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BINARY_NAME="rzn-tools"
USER_PREFIX="$HOME/.local"
DEFAULT_INSTALL_DIR="$USER_PREFIX/bin"
DEFAULT_ASSET_DIR="$USER_PREFIX/share/rzn-tools"
INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
FEATURES="${FEATURES:-full}"
BUILD_EXAMPLES="${BUILD_EXAMPLES:-1}"

say() { printf "%s\n" "$*"; }

die() {
  say "Error: $*" >&2
  exit 1
}

derive_asset_dir() {
  local install_dir="$1"
  if [[ -n "${ASSET_DIR:-}" ]]; then
    printf "%s\n" "$ASSET_DIR"
    return
  fi

  if [[ "$install_dir" == */bin ]]; then
    printf "%s/share/rzn-tools\n" "${install_dir%/bin}"
  else
    printf "%s\n" "$DEFAULT_ASSET_DIR"
  fi
}

copy_assets() {
  local asset_dir="$1"
  mkdir -p "$asset_dir"
  rm -rf "$asset_dir/resources" "$asset_dir/examples"
  mkdir -p "$asset_dir/resources" "$asset_dir/examples"
  cp -R "$ROOT_DIR/resources/." "$asset_dir/resources/"
  cp -R "$ROOT_DIR/examples/." "$asset_dir/examples/"
}

if ! command -v cargo >/dev/null 2>&1; then
  die "cargo not found. Install Rust (https://rustup.rs) and try again."
fi

say "Building ${BINARY_NAME} (features: ${FEATURES})..."
cd "$ROOT_DIR"

if [[ "$FEATURES" == "full" ]]; then
  cargo build --release -p rzn_tools_cli --features full
else
  cargo build --release -p rzn_tools_cli --features "$FEATURES"
fi

if [[ "$BUILD_EXAMPLES" == "1" ]]; then
  say "Compiling example binaries and validating bundled workflows..."
  cargo build --release -p rzn_tools_core --features "examples,full" --examples
  cargo test -p rzn_tools_core --test system_metadata_conformance
fi

BIN_PATH="$ROOT_DIR/target/release/$BINARY_NAME"
if [[ ! -x "$BIN_PATH" ]]; then
  die "build succeeded but binary not found at $BIN_PATH"
fi

ASSET_DIR="$(derive_asset_dir "$INSTALL_DIR")"

say "Installing binary to $INSTALL_DIR..."
say "Installing bundled workflows/examples to $ASSET_DIR..."
if mkdir -p "$INSTALL_DIR" 2>/dev/null && mkdir -p "$(dirname "$ASSET_DIR")" 2>/dev/null \
  && [[ -w "$INSTALL_DIR" && -w "$(dirname "$ASSET_DIR")" ]]; then
  cp "$BIN_PATH" "$INSTALL_DIR/$BINARY_NAME"
  chmod +x "$INSTALL_DIR/$BINARY_NAME"
  copy_assets "$ASSET_DIR"
else
  say "Install target not writable: $INSTALL_DIR"
  INSTALL_DIR="$DEFAULT_INSTALL_DIR"
  ASSET_DIR="$DEFAULT_ASSET_DIR"
  say "Falling back to user install dir: $INSTALL_DIR"
  say "Falling back to user asset dir: $ASSET_DIR"
  mkdir -p "$INSTALL_DIR"
  mkdir -p "$(dirname "$ASSET_DIR")"
  cp "$BIN_PATH" "$INSTALL_DIR/$BINARY_NAME"
  chmod +x "$INSTALL_DIR/$BINARY_NAME"
  copy_assets "$ASSET_DIR"
fi

if command -v "$BINARY_NAME" >/dev/null 2>&1; then
  say "Installed: $BINARY_NAME $("$BINARY_NAME" --version 2>/dev/null || true)"
else
  say "Installed to $INSTALL_DIR/$BINARY_NAME but it's not in PATH."
  say "Add this to your shell profile:"
  say "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi

say "Bundled workflows/examples installed at $ASSET_DIR"
say "Use '$BINARY_NAME workflows list' to inspect them or '$BINARY_NAME workflows sync --remote' to pull newer ones."
