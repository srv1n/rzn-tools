#!/usr/bin/env bash
set -euo pipefail

mode="${1:-quick}"

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$repo_root"

case "$mode" in
  quick)
    make fmt-check
    make check CARGO_ARGS="-p rzn_tools_cli"
    make check CARGO_ARGS="-p rzn_tools_mcp --features full"
    ;;
  full)
    make fmt-check
    make clippy CARGO_ARGS="--all-targets --all-features -- -D warnings"
    make test CARGO_ARGS="--workspace"
    RUSTDOCFLAGS="-D warnings" make doc CARGO_ARGS="--workspace"
    ;;
  release-cli)
    make build-release CARGO_ARGS="-p rzn_tools_cli --features full"
    ;;
  release-mcp)
    make build-release CARGO_ARGS="-p rzn_tools_mcp --features full"
    ;;
  *)
    printf 'Usage: %s [quick|full|release-cli|release-mcp]\n' "$0" >&2
    exit 2
    ;;
esac
