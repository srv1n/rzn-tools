#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

REPO="srv1n/rzn-tools"
BINARY_NAME="rzn-tools"
USER_PREFIX="${HOME}/.local"
DEFAULT_INSTALL_DIR="${USER_PREFIX}/bin"
DEFAULT_ASSET_DIR="${USER_PREFIX}/share/rzn-tools"
INSTALL_DIR="${INSTALL_DIR:-$DEFAULT_INSTALL_DIR}"
TEMP_DIR="${TMPDIR:-/tmp}/rzn-tools-install-$$"

print_header() {
  echo -e "${BLUE}=== $1 ===${NC}"
}

print_success() {
  echo -e "${GREEN}✓ $1${NC}"
}

print_warning() {
  echo -e "${YELLOW}⚠ $1${NC}"
}

print_error() {
  echo -e "${RED}✗ $1${NC}"
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

detect_platform() {
  local os arch

  case "$OSTYPE" in
    linux*) os="unknown-linux-gnu" ;;
    darwin*) os="apple-darwin" ;;
    *)
      print_error "Unsupported OS for shell install: $OSTYPE"
      exit 1
      ;;
  esac

  case "$(uname -m)" in
    x86_64|amd64) arch="x86_64" ;;
    aarch64|arm64) arch="aarch64" ;;
    *)
      print_error "Unsupported architecture: $(uname -m)"
      exit 1
      ;;
  esac

  printf "%s-%s\n" "$arch" "$os"
}

http_get() {
  local url="$1"
  local out="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -o "$out" "$url"
    return
  fi

  if command -v wget >/dev/null 2>&1; then
    wget -q -O "$out" "$url"
    return
  fi

  print_error "Neither curl nor wget is installed"
  exit 1
}

get_latest_version() {
  local api_url="https://api.github.com/repos/${REPO}/releases/latest"
  local raw

  if command -v curl >/dev/null 2>&1; then
    raw="$(curl -fsSL "$api_url")"
  elif command -v wget >/dev/null 2>&1; then
    raw="$(wget -qO- "$api_url")"
  else
    print_error "Neither curl nor wget is installed"
    exit 1
  fi

  local version
  version="$(printf "%s\n" "$raw" | grep '"tag_name"' | head -n1 | cut -d'"' -f4)"
  if [[ -z "$version" ]]; then
    print_error "Failed to resolve latest release tag"
    exit 1
  fi

  printf "%s\n" "$version"
}

download_payloads() {
  local target="$1"
  local version="$2"
  local binary_archive="rzn-tools-${version}-${target}.tar.gz"
  local workflow_archive="rzn-tools-workflows-${version}.tar.gz"

  rm -rf "$TEMP_DIR"
  mkdir -p "$TEMP_DIR"
  cd "$TEMP_DIR"

  print_header "Downloading release assets"
  http_get \
    "https://github.com/${REPO}/releases/download/${version}/${binary_archive}" \
    "$binary_archive"
  http_get \
    "https://github.com/${REPO}/releases/download/${version}/${workflow_archive}" \
    "$workflow_archive"

  print_success "Downloaded ${binary_archive}"
  print_success "Downloaded ${workflow_archive}"

  tar -xzf "$binary_archive"
  tar -xzf "$workflow_archive"

  if [[ ! -f "$BINARY_NAME" ]]; then
    print_error "Binary $BINARY_NAME not found in ${binary_archive}"
    exit 1
  fi

  if [[ ! -d "share/rzn-tools" ]]; then
    print_error "Workflow/icon assets not found in ${workflow_archive}"
    exit 1
  fi
}

install_payloads() {
  local asset_dir="$1"

  if mkdir -p "$INSTALL_DIR" 2>/dev/null && mkdir -p "$(dirname "$asset_dir")" 2>/dev/null \
    && [[ -w "$INSTALL_DIR" && -w "$(dirname "$asset_dir")" ]]; then
    cp "$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"
    rm -rf "$asset_dir"
    mkdir -p "$asset_dir"
    cp -R "share/rzn-tools/." "$asset_dir/"
    return
  fi

  print_header "Installing to ${INSTALL_DIR} and ${asset_dir} (sudo may prompt)"
  sudo mkdir -p "$INSTALL_DIR" "$(dirname "$asset_dir")"
  sudo cp "$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
  sudo chmod +x "$INSTALL_DIR/$BINARY_NAME"
  sudo rm -rf "$asset_dir"
  sudo mkdir -p "$asset_dir"
  sudo cp -R "share/rzn-tools/." "$asset_dir/"
}

verify_installation() {
  local asset_dir="$1"

  print_header "Verifying installation"
  if command -v "$BINARY_NAME" >/dev/null 2>&1; then
    print_success "$("$BINARY_NAME" --version 2>/dev/null || printf "%s\n" "$BINARY_NAME installed")"
  else
    print_warning "Binary installed to ${INSTALL_DIR}/${BINARY_NAME} but that directory is not in PATH"
    echo "Add this to your shell profile:"
    echo "export PATH=\"$INSTALL_DIR:\$PATH\""
  fi

  if [[ -d "$asset_dir/resources/systems" && -d "$asset_dir/resources/icons/connectors" && -d "$asset_dir/examples" ]]; then
    print_success "Installed bundled workflows/examples/icons to ${asset_dir}"
  else
    print_error "Bundled workflow/icon asset install looks incomplete at ${asset_dir}"
    exit 1
  fi
}

show_post_install() {
  print_header "Next steps"
  echo "  ${BINARY_NAME} --version"
  echo "  ${BINARY_NAME} workflows list"
  echo "  ${BINARY_NAME} workflows sync --remote"
  echo "  ${BINARY_NAME} setup"
}

cleanup() {
  rm -rf "$TEMP_DIR"
}

main() {
  local target version asset_dir

  print_header "rzn-tools installer"

  target="$(detect_platform)"
  version="$(get_latest_version)"
  asset_dir="$(derive_asset_dir "$INSTALL_DIR")"

  print_success "Detected platform: ${target}"
  print_success "Latest release: ${version}"
  print_success "Binary install dir: ${INSTALL_DIR}"
  print_success "Workflow asset dir: ${asset_dir}"

  download_payloads "$target" "$version"
  install_payloads "$asset_dir"
  verify_installation "$asset_dir"
  show_post_install
  cleanup
}

case "${1:-}" in
  --help|-h)
    echo "rzn-tools installer"
    echo
    echo "Usage: $0"
    echo
    echo "Environment variables:"
    echo "  INSTALL_DIR   Binary install directory (default: ~/.local/bin)"
    echo "  ASSET_DIR     Workflow/example asset directory"
    exit 0
    ;;
esac

main "$@"
