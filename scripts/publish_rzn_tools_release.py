#!/usr/bin/env python3
"""
Build + upload + register + publish the rzn-tools extension (`rzn-tools`).

This follows "Option B" (backend owns the catalog + signing; extension repos upload
artifacts + register releases).

Release rule:
  - a build is not done until the backend has been notified through the admin API
  - the standard release pass is to publish to both:
      local -> http://localhost:8082
      cloud -> https://cloud.rzn.ai
  - if any target fails, stop and report that exact target/error

Preferred env (scoped publisher flow):
  - RZN_BACKEND_BASE_URL (e.g. http://localhost:8082) for target env
  - RZN_PLUGIN_PRODUCT_ID
  - RZN_PUBLISHER_KEY

Legacy env (fallback admin flow):
  - RZN_PLATFORM_ADMIN_TOKEN (bearer token; dev can increase TTL)
  - R2_PLUGINS_ACCESS_KEY_ID
  - R2_PLUGINS_SECRET_ACCESS_KEY
  - R2_PLUGINS_BUCKET
  - R2_PLUGINS_ENDPOINT (e.g. https://<accountid>.r2.cloudflarestorage.com)

Env (optional):
  - RZN_BACKEND_BASE_URL_LOCAL (default: http://localhost:8082)
  - RZN_BACKEND_BASE_URL_CLOUD (default: https://cloud.rzn.ai)
  - RZN_BACKEND_BASE_URL_PROD (legacy alias for cloud)
  - RZN_PLATFORM_ADMIN_TOKEN_LOCAL
  - RZN_PLATFORM_ADMIN_TOKEN_CLOUD
  - RZN_PLATFORM_ADMIN_TOKEN_PROD (legacy alias for cloud)
  - RZN_PLUGIN_PRODUCT_ID_LOCAL / RZN_PUBLISHER_KEY_LOCAL
  - RZN_PLUGIN_PRODUCT_ID_CLOUD / RZN_PUBLISHER_KEY_CLOUD
  - RZN_PLUGIN_PRODUCT_ID_PROD / RZN_PUBLISHER_KEY_PROD (legacy aliases for cloud)
  - R2_PLUGINS_REGION (default: auto)
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


def sh(cmd: list[str], *, env: dict | None = None) -> None:
    subprocess.run(cmd, check=True, env=env)


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


def http_post_json(url: str, token: str, payload: dict) -> dict:
    return http_request_json(
        "POST",
        url,
        headers={"Authorization": f"Bearer {token}"},
        payload=payload,
    )


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


def aws_env_from_r2() -> dict:
    access_key = os.environ.get("R2_PLUGINS_ACCESS_KEY_ID", "").strip()
    secret_key = os.environ.get("R2_PLUGINS_SECRET_ACCESS_KEY", "").strip()
    region = os.environ.get("R2_PLUGINS_REGION", "auto").strip()
    if not access_key or not secret_key:
        raise RuntimeError("missing R2_PLUGINS_ACCESS_KEY_ID / R2_PLUGINS_SECRET_ACCESS_KEY")
    env = os.environ.copy()
    env["AWS_ACCESS_KEY_ID"] = access_key
    env["AWS_SECRET_ACCESS_KEY"] = secret_key
    env["AWS_DEFAULT_REGION"] = region
    return env


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
        if name == "env":
            if name not in seen:
                seen.add(name)
                targets.append((name, resolve_backend_base(name)))
            continue
        canonical = "cloud" if name == "prod" else name
        if canonical in {"local", "cloud"}:
            if canonical not in seen:
                seen.add(canonical)
                targets.append((canonical, resolve_backend_base(canonical)))
            continue
        raise RuntimeError(
            f"unsupported target '{name}' (use env, local, cloud, prod, or all)"
        )
    return targets


def resolve_backend_base(target_name: str) -> str:
    if target_name == "env":
        backend_base = os.environ.get("RZN_BACKEND_BASE_URL", "").strip().rstrip("/")
        if not backend_base:
            raise RuntimeError(
                "missing RZN_BACKEND_BASE_URL for target env (e.g. http://localhost:8082)"
            )
        return backend_base
    if target_name == "local":
        return (
            os.environ.get("RZN_BACKEND_BASE_URL_LOCAL", "").strip().rstrip("/")
            or DEFAULT_LOCAL_BACKEND
        )
    if target_name in {"cloud", "prod"}:
        return (
            os.environ.get("RZN_BACKEND_BASE_URL_CLOUD", "").strip().rstrip("/")
            or os.environ.get("RZN_BACKEND_BASE_URL_PROD", "").strip().rstrip("/")
            or DEFAULT_CLOUD_BACKEND
        )
    raise RuntimeError(f"unsupported backend target '{target_name}'")


def resolve_admin_token(target_name: str) -> str:
    if target_name == "local":
        token = os.environ.get("RZN_PLATFORM_ADMIN_TOKEN_LOCAL", "").strip()
        if token:
            return token
    if target_name in {"cloud", "prod"}:
        token = os.environ.get("RZN_PLATFORM_ADMIN_TOKEN_CLOUD", "").strip()
        if token:
            return token
        token = os.environ.get("RZN_PLATFORM_ADMIN_TOKEN_PROD", "").strip()
        if token:
            return token
    token = os.environ.get("RZN_PLATFORM_ADMIN_TOKEN", "").strip()
    if token:
        return token
    if target_name == "env":
        raise RuntimeError("missing RZN_PLATFORM_ADMIN_TOKEN")
    raise RuntimeError(
        f"missing admin token for target {target_name}; set target-specific env or RZN_PLATFORM_ADMIN_TOKEN"
    )


def resolve_public_base(target_name: str, backend_base: str) -> str:
    if target_name == "env":
        return (
            os.environ.get("RZN_PLUGIN_PUBLIC_BASE_URL", "").strip().rstrip("/") or backend_base
        )
    if target_name == "local":
        return (
            os.environ.get("RZN_PLUGIN_PUBLIC_BASE_URL_LOCAL", "").strip().rstrip("/")
            or backend_base
        )
    if target_name in {"cloud", "prod"}:
        return (
            os.environ.get("RZN_PLUGIN_PUBLIC_BASE_URL_CLOUD", "").strip().rstrip("/")
            or os.environ.get("RZN_PLUGIN_PUBLIC_BASE_URL_PROD", "").strip().rstrip("/")
            or DEFAULT_CLOUD_BACKEND
        )
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
    elif target_name in {"cloud", "prod"}:
        product_id = (
            os.environ.get("RZN_PLUGIN_PRODUCT_ID_CLOUD", "").strip()
            or os.environ.get("RZN_PLUGIN_PRODUCT_ID_PROD", "").strip()
            or os.environ.get("RZN_PLUGIN_PRODUCT_ID", "").strip()
        )
        publisher_key = (
            os.environ.get("RZN_PUBLISHER_KEY_CLOUD", "").strip()
            or os.environ.get("RZN_PUBLISHER_KEY_PROD", "").strip()
            or os.environ.get("RZN_PUBLISHER_KEY", "").strip()
        )
    else:
        product_id = os.environ.get("RZN_PLUGIN_PRODUCT_ID", "").strip()
        publisher_key = os.environ.get("RZN_PUBLISHER_KEY", "").strip()
    if product_id and publisher_key:
        return product_id, publisher_key
    return None


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Build + upload + register + publish rzn-tools to backend Option B."
    )
    ap.add_argument(
        "--config",
        default="scripts/plugins/config/rzn-tools.json",
        help="Plugin config JSON path",
    )
    ap.add_argument("--platform", default="macos_arm64", help="Platform key")
    ap.add_argument("--channel", default="stable", choices=["stable", "beta", "nightly"])
    ap.add_argument(
        "--catalog-version",
        default="",
        help="Optional RFC3339 catalog version; default uses backend now()",
    )
    ap.add_argument("--skip-build", action="store_true", help="Skip build steps")
    ap.add_argument("--skip-upload", action="store_true", help="Skip R2 upload")
    ap.add_argument("--skip-publish", action="store_true", help="Skip catalog publish")
    ap.add_argument(
        "--targets",
        default="env",
        help="Comma-separated publish targets: env, local, cloud, prod, or all",
    )
    args = ap.parse_args()

    root = Path(__file__).resolve().parents[1]
    config_path = (root / args.config).resolve()
    config = load_config(config_path)

    plugin_id = str(config["id"]).strip()
    version = str(config["version"]).strip()
    maybe_load_seeded_publisher_env(root, plugin_id)
    r2_bucket = os.environ.get("R2_PLUGINS_BUCKET", "").strip()
    r2_endpoint = os.environ.get("R2_PLUGINS_ENDPOINT", "").strip()
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
    needs_legacy_upload = any(
        resolve_publisher_credentials(target_name) is None for target_name, _ in targets
    )

    if needs_legacy_upload and not args.skip_upload:
        if not r2_bucket:
            raise RuntimeError("missing R2_PLUGINS_BUCKET")
        if not r2_endpoint:
            raise RuntimeError("missing R2_PLUGINS_ENDPOINT")
        env = aws_env_from_r2()
        sh(["aws", "configure", "set", "default.s3.addressing_style", "path"], env=env)
        sh(
            [
                "aws",
                "s3api",
                "put-object",
                "--endpoint-url",
                r2_endpoint,
                "--bucket",
                r2_bucket,
                "--key",
                artifact_key,
                "--body",
                str(zip_path),
                "--content-type",
                "application/zip",
            ],
            env=env,
        )

    for target_name, backend_base in targets:
        public_base = resolve_public_base(target_name, backend_base)
        print(f"[target:{target_name}] backend={backend_base}")
        try:
            publisher_creds = resolve_publisher_credentials(target_name)
            if publisher_creds:
                if args.skip_upload:
                    raise RuntimeError(
                        "--skip-upload is not supported with the scoped publisher flow"
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
                continue

            admin_token = resolve_admin_token(target_name)
            if not r2_bucket:
                raise RuntimeError("missing R2_PLUGINS_BUCKET")
            if not r2_endpoint:
                raise RuntimeError("missing R2_PLUGINS_ENDPOINT")

            reg = http_post_json(
                f"{backend_base}/admin/plugins/releases",
                admin_token,
                {
                    "plugin_id": plugin_id,
                    "version": version,
                    "platform": args.platform,
                    "artifact_key": artifact_key,
                    "artifact_sha256": digest,
                    "notes": "rzn-tools publish",
                },
            )
            print(f"[target:{target_name}] registered:", reg)

            if not args.skip_publish:
                payload = {"channel": args.channel, "base_url": f"{public_base}/plugins/artifacts"}
                if args.catalog_version.strip():
                    payload["catalog_version"] = args.catalog_version.strip()
                pub = http_post_json(
                    f"{backend_base}/admin/plugins/catalog/publish",
                    admin_token,
                    payload,
                )
                print(f"[target:{target_name}] published:", pub)
                verify_public_release(
                    public_base,
                    channel=args.channel,
                    plugin_id=plugin_id,
                    version=version,
                    artifact_key=artifact_key,
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
