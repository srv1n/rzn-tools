#!/usr/bin/env python3
"""
Build + upload + register + publish the rzn-tools extension (`rzn-tools`).

The backend owns the catalog and signing; this repository uploads the artifact and
registers the release through the scoped publisher API.

Release rule:
  - a build is not done until the backend has been notified through the publisher API
  - the standard release pass is to publish to both:
      local -> http://localhost:8082
      cloud -> https://cloud.rzn.ai
  - if any target fails, stop and report that exact target/error

Env:
  - RZN_BACKEND_BASE_URL_LOCAL (default: http://localhost:8082)
  - RZN_BACKEND_BASE_URL_CLOUD (default: https://cloud.rzn.ai)
  - RZN_PLUGIN_PRODUCT_ID / RZN_PUBLISHER_KEY
  - RZN_PLUGIN_PRODUCT_ID_LOCAL / RZN_PUBLISHER_KEY_LOCAL
  - RZN_PLUGIN_PRODUCT_ID_CLOUD / RZN_PUBLISHER_KEY_CLOUD
  - R2_PLUGINS_PREFIX (default: plugins)
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

DEFAULT_LOCAL_BACKEND = "http://localhost:8082"
DEFAULT_CLOUD_BACKEND = "https://cloud.rzn.ai"


def sh(cmd: list[str]) -> None:
    subprocess.run(cmd, check=True)


def sha256_hex(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def http_request_json(
    method: str,
    url: str,
    *,
    headers: dict[str, str] | None = None,
    payload: dict | None = None,
) -> dict:
    body = None if payload is None else json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(url, method=method, data=body)
    if payload is not None:
        req.add_header("Content-Type", "application/json")
    for key, value in (headers or {}).items():
        req.add_header(key, value)
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            raw = resp.read().decode("utf-8", errors="replace")
            return json.loads(raw) if raw.strip() else {}
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"{method} {url} failed: {e.code} {raw}") from None


def http_request_bytes(
    method: str,
    url: str,
    *,
    headers: dict[str, str] | None = None,
) -> bytes:
    req = urllib.request.Request(url, method=method)
    for key, value in (headers or {}).items():
        req.add_header(key, value)
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return resp.read()
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"{method} {url} failed: {e.code} {raw}") from None


def load_config(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def load_env_file(path: Path) -> None:
    if not path.exists():
        return
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        os.environ.setdefault(key.strip(), value.strip().strip('"'))


def maybe_load_seeded_publisher_env(root: Path, plugin_id: str) -> None:
    candidates = [
        root.parent / "backend" / ".secrets" / "plugin-publishers" / f"{plugin_id}.env",
        root / ".secrets" / f"plugin-publisher-{plugin_id}.env",
        root / ".secrets" / "plugin-publisher.env",
    ]
    for candidate in candidates:
        load_env_file(candidate)


def upload_presigned(
    upload_url: str, zip_path: Path, *, headers: dict[str, str] | None = None
) -> None:
    req = urllib.request.Request(upload_url, method="PUT", data=zip_path.read_bytes())
    req.add_header("Content-Type", "application/zip")
    for key, value in (headers or {}).items():
        req.add_header(key, value)
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            resp.read()
    except urllib.error.HTTPError as e:
        raw = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"PUT {upload_url} failed: {e.code} {raw}") from None


class NoRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def artifact_rel_path(artifact_key: str, prefix: str) -> str:
    normalized = artifact_key.strip().lstrip("/")
    normalized_prefix = prefix.strip().strip("/")
    if normalized_prefix and normalized.startswith(f"{normalized_prefix}/"):
        return normalized[len(normalized_prefix) + 1 :]
    return normalized


def probe_artifact_endpoint(url: str) -> int:
    req = urllib.request.Request(url, method="GET")
    opener = urllib.request.build_opener(NoRedirectHandler)
    try:
        with opener.open(req, timeout=60) as resp:
            resp.read(1)
            return getattr(resp, "status", resp.getcode())
    except urllib.error.HTTPError as e:
        if e.code in {301, 302, 303, 307, 308}:
            return e.code
        raw = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"GET {url} failed: {e.code} {raw}") from None


def verify_public_release(
    public_base: str,
    *,
    channel: str,
    plugin_id: str,
    version: str,
    artifact_key: str,
    r2_prefix: str,
) -> None:
    catalog_url = f"{public_base}/plugins/index.json?channel={channel}"
    sig_url = f"{public_base}/plugins/index.sig?channel={channel}"
    catalog = http_request_json("GET", catalog_url)
    sig = http_request_bytes("GET", sig_url).strip()
    if not sig:
        raise RuntimeError(f"empty signature served from {sig_url}")

    plugins = catalog.get("plugins")
    if not isinstance(plugins, list):
        raise RuntimeError(f"catalog from {public_base} is missing plugins[]")

    plugin_entry = next(
        (
            item
            for item in plugins
            if item.get("id") == plugin_id and item.get("version") == version
        ),
        None,
    )
    if plugin_entry is None:
        raise RuntimeError(
            f"catalog from {public_base} does not expose {plugin_id}@{version}"
        )

    rel_path = artifact_rel_path(artifact_key, r2_prefix)
    expected_url = f"{public_base}/plugins/artifacts/{rel_path}"
    platforms = plugin_entry.get("platforms") or []
    platform_entry = next((item for item in platforms if item.get("url") == expected_url), None)
    if platform_entry is None:
        raise RuntimeError(
            f"catalog from {public_base} does not expose artifact url {expected_url}"
        )

    artifact_status = probe_artifact_endpoint(expected_url)
    if artifact_status not in {200, 301, 302, 303, 307, 308}:
        raise RuntimeError(
            f"artifact probe for {expected_url} returned unexpected status {artifact_status}"
        )


def resolve_targets(spec: str) -> list[tuple[str, str]]:
    requested = [part.strip().lower() for part in spec.split(",") if part.strip()]
    if not requested:
        raise RuntimeError("no publish targets requested")

    targets: list[tuple[str, str]] = []
    seen: set[str] = set()
    for name in requested:
        if name == "all":
            for expanded in ("local", "cloud"):
                if expanded not in seen:
                    seen.add(expanded)
                    targets.append((expanded, resolve_backend_base(expanded)))
            continue
        if name in {"local", "cloud"}:
            if name not in seen:
                seen.add(name)
                targets.append((name, resolve_backend_base(name)))
            continue
        raise RuntimeError(
            f"unsupported target '{name}' (use local, cloud, or all)"
        )
    return targets


def resolve_backend_base(target_name: str) -> str:
    if target_name == "local":
        return (
            os.environ.get("RZN_BACKEND_BASE_URL_LOCAL", "").strip().rstrip("/")
            or DEFAULT_LOCAL_BACKEND
        )
    if target_name == "cloud":
        return os.environ.get("RZN_BACKEND_BASE_URL_CLOUD", "").strip().rstrip("/") or DEFAULT_CLOUD_BACKEND
    raise RuntimeError(f"unsupported backend target '{target_name}'")


def resolve_public_base(target_name: str, backend_base: str) -> str:
    if target_name == "local":
        return (
            os.environ.get("RZN_PLUGIN_PUBLIC_BASE_URL_LOCAL", "").strip().rstrip("/")
            or backend_base
        )
    if target_name == "cloud":
        return os.environ.get("RZN_PLUGIN_PUBLIC_BASE_URL_CLOUD", "").strip().rstrip("/") or DEFAULT_CLOUD_BACKEND
    raise RuntimeError(f"unsupported public target '{target_name}'")


def resolve_publisher_credentials(target_name: str) -> tuple[str, str] | None:
    if target_name == "local":
        product_id = (
            os.environ.get("RZN_PLUGIN_PRODUCT_ID_LOCAL", "").strip()
            or os.environ.get("RZN_PLUGIN_PRODUCT_ID", "").strip()
        )
        publisher_key = (
            os.environ.get("RZN_PUBLISHER_KEY_LOCAL", "").strip()
            or os.environ.get("RZN_PUBLISHER_KEY", "").strip()
        )
    elif target_name == "cloud":
        product_id = (
            os.environ.get("RZN_PLUGIN_PRODUCT_ID_CLOUD", "").strip()
            or os.environ.get("RZN_PLUGIN_PRODUCT_ID", "").strip()
        )
        publisher_key = (
            os.environ.get("RZN_PUBLISHER_KEY_CLOUD", "").strip()
            or os.environ.get("RZN_PUBLISHER_KEY", "").strip()
        )
    else:
        raise RuntimeError(f"unsupported publisher target '{target_name}'")
    if product_id and publisher_key:
        return product_id, publisher_key
    return None


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Build, upload, register, and publish rzn-tools through the scoped publisher API."
    )
    ap.add_argument(
        "--config",
        default="scripts/plugins/config/rzn-tools.json",
        help="Plugin config JSON path",
    )
    ap.add_argument("--platform", default="macos_arm64", help="Platform key")
    ap.add_argument("--channel", default="stable", choices=["stable", "beta", "nightly"])
    ap.add_argument("--skip-build", action="store_true", help="Skip build steps")
    ap.add_argument("--skip-publish", action="store_true", help="Skip catalog publish")
    ap.add_argument(
        "--targets",
        default="all",
        help="Comma-separated publish targets: local, cloud, or all",
    )
    args = ap.parse_args()

    root = Path(__file__).resolve().parents[1]
    config_path = (root / args.config).resolve()
    config = load_config(config_path)

    plugin_id = str(config["id"]).strip()
    version = str(config["version"]).strip()
    maybe_load_seeded_publisher_env(root, plugin_id)
    r2_prefix = os.environ.get("R2_PLUGINS_PREFIX", "plugins").strip().strip("/")
    targets = resolve_targets(args.targets)

    # 1) Build artifact zip
    if not args.skip_build:
        sh(["make", "plugins-build-rzn-tools-macos-arm64"])

    zip_name = f"{plugin_id}-{version}-{args.platform}.zip"
    zip_path = root / "dist/plugins" / plugin_id / version / args.platform / zip_name
    if not zip_path.exists():
        raise RuntimeError(f"missing built zip: {zip_path}")

    digest = sha256_hex(zip_path)
    artifact_key = f"{r2_prefix}/{plugin_id}/{version}/{args.platform}/{zip_name}"

    for target_name, backend_base in targets:
        public_base = resolve_public_base(target_name, backend_base)
        print(f"[target:{target_name}] backend={backend_base}")
        try:
            publisher_creds = resolve_publisher_credentials(target_name)
            if not publisher_creds:
                raise RuntimeError(
                    f"missing scoped publisher credentials for {target_name}; "
                    "set RZN_PLUGIN_PRODUCT_ID and RZN_PUBLISHER_KEY "
                    "or their target-specific variants"
                )
            product_id, publisher_key = publisher_creds
            headers = {"x-rzn-publisher-key": publisher_key}
            release = http_request_json(
                "POST",
                f"{backend_base}/publisher/products/{product_id}/releases",
                headers=headers,
                payload={"version": version, "platform": args.platform},
            )
            release_data = release.get("data", release)
            release_id = str(release_data["id"]).strip()
            upload = http_request_json(
                "POST",
                f"{backend_base}/publisher/releases/{release_id}/upload-session",
                headers=headers,
            )
            upload_data = upload.get("data", upload)
            upload_url = str(upload_data["upload_url"])
            upload_headers = headers if "/publisher/releases/" in upload_url else None
            upload_presigned(upload_url, zip_path, headers=upload_headers)
            finalized = http_request_json(
                "POST",
                f"{backend_base}/publisher/releases/{release_id}/finalize",
                headers=headers,
                payload={
                    "artifact_sha256": digest,
                    "release_notes": "rzn-tools publish",
                    "metadata": {"artifact_key": upload_data.get("artifact_key")},
                },
            )
            print(f"[target:{target_name}] finalized:", finalized)
            if not args.skip_publish:
                published = http_request_json(
                    "POST",
                    f"{backend_base}/publisher/releases/{release_id}/publish",
                    headers=headers,
                    payload={"channel": args.channel},
                )
                print(f"[target:{target_name}] published:", published)
                verify_public_release(
                    public_base,
                    channel=args.channel,
                    plugin_id=plugin_id,
                    version=version,
                    artifact_key=str(upload_data.get("artifact_key") or artifact_key),
                    r2_prefix=r2_prefix,
                )
                print(f"[target:{target_name}] verified catalog + artifact serving")
        except Exception as exc:
            raise RuntimeError(
                f"release failed for target {target_name} ({backend_base}): {exc}"
            ) from exc

    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as e:
        print(f"error: {e}", file=sys.stderr)
        raise
