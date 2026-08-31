#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BUILD_ROOT="${BUILD_ROOT:-$ROOT_DIR/target/sidecars}"
STAGING_DIR="$ROOT_DIR/target/sidecar-staging"
BINARY="$STAGING_DIR/rzn-tools"

cd "$ROOT_DIR"
rm -rf "$STAGING_DIR"
mkdir -p "$STAGING_DIR"

make build-release CARGO_ARGS="-p rzn_tools_mcp --no-default-features --features server-full"
cp target/release/rzn-tools-mcp "$BINARY"
chmod 0555 "$BINARY"

if command -v sha256sum >/dev/null 2>&1; then
  digest="$(sha256sum "$BINARY" | awk '{print $1}')"
else
  digest="$(shasum -a 256 "$BINARY" | awk '{print $1}')"
fi

artifact_dir="$BUILD_ROOT/$digest"
if [[ -e "$artifact_dir" ]]; then
  printf '%s\n' "Immutable sidecar already exists: $artifact_dir"
  exit 0
fi
mkdir -p "$artifact_dir"
cp "$BINARY" "$artifact_dir/rzn-tools"
chmod 0555 "$artifact_dir/rzn-tools"

printf '%s  %s\n' "$digest" rzn-tools > "$artifact_dir/rzn-tools.sha256"

python3 - "$artifact_dir/manifest.json" "$digest" <<'PY'
import json
import pathlib
import platform
import sys

manifest_path = pathlib.Path(sys.argv[1])
digest = sys.argv[2]
manifest = {
    "schema_version": 1,
    "artifact": "rzn-tools-sidecar",
    "binary": "rzn-tools",
    "sha256": digest,
    "protocol": "mcp-stdio",
    "protocol_version": "2025-03-26",
    "capability_version": "rzn-tools-mcp-capabilities-v1",
    "handshake": "initialize",
    "transport": "jsonrpc-2.0-lines",
    "max_frame_bytes": 4194304,
    "features": "server-full",
    "target": f"{platform.machine()}-{platform.system().lower()}",
    "install_layout": f"/opt/rzn/sidecars/rzn-tools/{digest}/rzn-tools",
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
PY

printf '%s\n' "Sidecar binary: $artifact_dir/rzn-tools"
printf '%s\n' "Sidecar digest: $artifact_dir/rzn-tools.sha256"
printf '%s\n' "Sidecar manifest: $artifact_dir/manifest.json"
