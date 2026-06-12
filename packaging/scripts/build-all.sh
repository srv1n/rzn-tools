#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

PACKAGE_NAME="rzn-tools"
VERSION=${VERSION:-$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="rzn_tools_cli") | .version')}
BUILD_DIR="target/releases"
WORKSPACE_ROOT="$(pwd)"

declare -a TARGETS=(
  "x86_64-unknown-linux-gnu"
  "x86_64-unknown-linux-musl"
  "aarch64-unknown-linux-gnu"
  "aarch64-unknown-linux-musl"
  "x86_64-pc-windows-gnu"
  "armv7-unknown-linux-gnueabihf"
)

declare -a MACOS_TARGETS=(
  "x86_64-apple-darwin"
  "aarch64-apple-darwin"
)

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

check_prerequisites() {
  print_header "Checking prerequisites"

  command -v cross >/dev/null 2>&1 || {
    print_error "cross is not installed. Install with: cargo install cross --git https://github.com/cross-rs/cross"
    exit 1
  }

  command -v docker >/dev/null 2>&1 || {
    print_error "Docker is not installed"
    exit 1
  }

  docker info >/dev/null 2>&1 || {
    print_error "Docker daemon is not running"
    exit 1
  }

  command -v jq >/dev/null 2>&1 || {
    print_error "jq is required for version detection"
    exit 1
  }

  print_success "Prerequisites look good"
}

prepare_build_dir() {
  print_header "Preparing build directory"
  rm -rf "$BUILD_DIR"
  mkdir -p "$BUILD_DIR"
  print_success "Using $BUILD_DIR"
}

validate_assets() {
  print_header "Validating bundled workflow assets"
  cargo build --release -p rzn_tools_core --features "examples,full" --examples
  cargo test -p rzn_tools_core --test system_metadata_conformance
  print_success "Examples compile and workflow assets validate"
}

create_workflow_bundle() {
  local bundle_root="${BUILD_DIR}/workflow-bundle/share/rzn-tools"
  local archive_name="${PACKAGE_NAME}-workflows-v${VERSION}.tar.gz"

  print_header "Creating workflow bundle"
  rm -rf "${BUILD_DIR}/workflow-bundle"
  mkdir -p "$bundle_root"
  cp -R resources "$bundle_root/"
  cp -R examples "$bundle_root/"

  (
    cd "${BUILD_DIR}/workflow-bundle"
    tar -czf "../${archive_name}" share
  )

  print_success "Created ${archive_name}"
}

create_archive() {
  local target="$1"
  local target_dir="$2"
  local archive_name="${PACKAGE_NAME}-v${VERSION}-${target}"

  print_header "Creating archive for ${target}"

  (
    cd "$target_dir"
    if [[ "$target" == *"windows"* ]]; then
      zip -q "../${archive_name}.zip" "${PACKAGE_NAME}.exe"
    else
      tar -czf "../${archive_name}.tar.gz" "$PACKAGE_NAME"
    fi
  )

  print_success "Created ${archive_name}"
}

build_target() {
  local target="$1"
  local binary_name="${PACKAGE_NAME}"

  if [[ "$target" == *"windows"* ]]; then
    binary_name="${PACKAGE_NAME}.exe"
  fi

  print_header "Building ${target}"
  if cross build --release --target "$target" -p rzn_tools_cli --features full; then
    local target_dir="${BUILD_DIR}/${target}"
    mkdir -p "$target_dir"
    cp "target/${target}/release/${binary_name}" "${target_dir}/"
    create_archive "$target" "$target_dir"
  else
    print_warning "Skipping ${target} after build failure"
  fi
}

build_macos() {
  if [[ "$OSTYPE" != "darwin"* ]]; then
    print_warning "Skipping macOS targets because host is not macOS"
    return
  fi

  for target in "${MACOS_TARGETS[@]}"; do
    print_header "Building ${target}"
    rustup target add "$target" >/dev/null 2>&1 || true
    if cargo build --release --target "$target" -p rzn_tools_cli --features full; then
      local target_dir="${BUILD_DIR}/${target}"
      mkdir -p "$target_dir"
      cp "target/${target}/release/${PACKAGE_NAME}" "${target_dir}/"
      create_archive "$target" "$target_dir"
    else
      print_warning "Skipping ${target} after build failure"
    fi
  done
}

generate_checksums() {
  print_header "Generating checksums"
  (
    cd "$BUILD_DIR"
    if command -v sha256sum >/dev/null 2>&1; then
      sha256sum *.{tar.gz,zip} 2>/dev/null > checksums.txt || true
    else
      shasum -a 256 *.{tar.gz,zip} 2>/dev/null > checksums.txt || true
    fi
  )
  print_success "Wrote ${BUILD_DIR}/checksums.txt"
}

list_artifacts() {
  print_header "Artifacts"
  ls -lh "$BUILD_DIR"/*.{tar.gz,zip} 2>/dev/null || true
  [[ -f "${BUILD_DIR}/checksums.txt" ]] && cat "${BUILD_DIR}/checksums.txt"
}

main() {
  print_header "Building rzn-tools v${VERSION} release artifacts"
  check_prerequisites
  prepare_build_dir
  validate_assets
  create_workflow_bundle

  for target in "${TARGETS[@]}"; do
    build_target "$target"
  done

  build_macos
  generate_checksums
  list_artifacts

  print_success "Release artifacts are in ${BUILD_DIR}"
}

main "$@"
