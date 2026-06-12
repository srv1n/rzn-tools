#!/usr/bin/env python3
"""
Normalize GitHub release titles so legacy `vX.Y.Z` titles use the `rzn-tools` name.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

API_BASE = "https://api.github.com"


class GitHubReleaseError(RuntimeError):
    """Raised when GitHub release operations fail."""


def github_request(
    method: str,
    url: str,
    *,
    token: str | None,
    payload: dict | None = None,
) -> object:
    data = None if payload is None else json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(url, method=method, data=data)
    request.add_header("Accept", "application/vnd.github+json")
    request.add_header("X-GitHub-Api-Version", "2022-11-28")
    if payload is not None:
        request.add_header("Content-Type", "application/json")
    if token:
        request.add_header("Authorization", f"Bearer {token}")

    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            raw = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        raise GitHubReleaseError(f"{method} {url} failed: {exc.code} {detail}") from None

    return json.loads(raw) if raw.strip() else {}


def iter_releases(repo: str, token: str | None) -> list[dict]:
    releases: list[dict] = []
    page = 1
    while True:
        query = urllib.parse.urlencode({"per_page": 100, "page": page})
        url = f"{API_BASE}/repos/{repo}/releases?{query}"
        batch = github_request("GET", url, token=token)
        assert isinstance(batch, list)
        if not batch:
            break
        releases.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    return releases


def desired_release_name(tag_name: str) -> str:
    return f"rzn-tools {tag_name}"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Retitle legacy GitHub releases.")
    parser.add_argument(
        "--repo",
        default="srv1n/rzn-tools",
        help="GitHub repository in owner/name form",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="Apply changes instead of printing the plan",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    token = os.environ.get("GITHUB_TOKEN", "").strip() or None

    try:
        releases = iter_releases(args.repo, token)
        planned = [
            release
            for release in releases
            if (release.get("name") or "") != desired_release_name(str(release["tag_name"]))
        ]

        if not planned:
            print("All GitHub release titles are already normalized.")
            return 0

        for release in planned:
            release_id = release["id"]
            tag_name = str(release["tag_name"])
            current_name = release.get("name") or ""
            next_name = desired_release_name(tag_name)

            if not args.apply:
                print(f"[dry-run] release {release_id}: `{current_name}` -> `{next_name}`")
                continue

            if token is None:
                raise GitHubReleaseError("GITHUB_TOKEN is required to update release titles")

            url = f"{API_BASE}/repos/{args.repo}/releases/{release_id}"
            github_request("PATCH", url, token=token, payload={"name": next_name})
            print(f"Updated `{tag_name}`: `{current_name}` -> `{next_name}`")

        if not args.apply:
            print("Run again with --apply to update the release titles on GitHub.")
        return 0
    except GitHubReleaseError as exc:
        print(f"[retitle-releases] {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
