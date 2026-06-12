#!/usr/bin/env bash
set -euo pipefail

mode="${1:-quick}"

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$repo_root"

case "$mode" in
  quick)
    cargo fmt --all -- --check
    cargo check -p rzn_tools_cli
    cargo check -p rzn_tools_mcp --features full
    ;;
  full)
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings
    cargo test --workspace
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
    ;;
  release-cli)
    cargo build --release -p rzn_tools_cli --features full
    ;;
  release-mcp)
    cargo build --release -p rzn_tools_mcp --features full
    ;;
  *)
    printf 'Usage: %s [quick|full|release-cli|release-mcp]\n' "$0" >&2
    exit 2
    ;;
esac
