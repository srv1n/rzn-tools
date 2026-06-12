#!/usr/bin/env python3
"""
Create and push a release tag that triggers the GitHub release workflow.

This script is intentionally strict by default:
  - working tree must be clean
  - current branch must be `main`
  - local `main` must match `origin/main`
  - the requested tag must not already exist locally or remotely
  - the version must match across the releasable workspace crates
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RELEASE_CRATES = ("rzn_tools_cli", "rzn_tools_core", "rzn_tools_mcp")


class ReleaseError(RuntimeError):
    """Raised when release preconditions are not met."""


def run(*args: str) -> str:
    proc = subprocess.run(
        args,
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        stderr = proc.stderr.strip()
        stdout = proc.stdout.strip()
        detail = stderr or stdout or f"command failed with exit code {proc.returncode}"
        raise ReleaseError(f"{' '.join(args)}: {detail}")
    return proc.stdout.strip()


def git(*args: str) -> str:
    return run("git", *args)


def load_versions() -> dict[str, str]:
    versions: dict[str, str] = {}
    for crate in RELEASE_CRATES:
        cargo_toml = ROOT / crate / "Cargo.toml"
        data = tomllib.loads(cargo_toml.read_text(encoding="utf-8"))
        versions[crate] = str(data["package"]["version"])
    return versions


def normalize_version(raw: str) -> str:
    version = raw.strip()
    if not version:
        raise ReleaseError("release version cannot be empty")
    return version[1:] if version.startswith("v") else version


def ensure_clean_worktree() -> None:
    status = git("status", "--porcelain")
    if status:
        raise ReleaseError(
            "working tree is dirty; commit or stash changes first, or pass --allow-dirty"
        )


def ensure_branch(expected_branch: str) -> None:
    current = git("rev-parse", "--abbrev-ref", "HEAD")
    if current != expected_branch:
        raise ReleaseError(
            f"release must be cut from `{expected_branch}`; current branch is `{current}`"
        )


def ensure_remote_sync(remote: str, branch: str) -> None:
    git("fetch", "--quiet", "--tags", remote, branch)

    head = git("rev-parse", "HEAD")
    remote_ref = f"{remote}/{branch}"
    remote_head = git("rev-parse", remote_ref)
    if head != remote_head:
        raise ReleaseError(
            f"local HEAD ({head[:7]}) does not match {remote_ref} ({remote_head[:7]}). "
            "Push/fast-forward main first, then tag the exact commit you want to ship."
        )


def ensure_tag_is_free(tag: str, remote: str) -> None:
    try:
        git("rev-parse", "--verify", f"refs/tags/{tag}")
    except ReleaseError:
        pass
    else:
        raise ReleaseError(f"tag `{tag}` already exists locally")

    remote_match = git("ls-remote", "--tags", remote, f"refs/tags/{tag}")
    if remote_match:
        raise ReleaseError(f"tag `{tag}` already exists on {remote}")


def latest_release_tag() -> str | None:
    tags = [
        tag.strip()
        for tag in git("tag", "--list", "v*", "--sort=-v:refname").splitlines()
        if tag.strip()
    ]
    return tags[0] if tags else None


def commit_count_since(tag: str | None) -> int:
    range_spec = f"{tag}..HEAD" if tag else "HEAD"
    return int(run("git", "rev-list", "--count", range_spec))


def resolve_version(requested_version: str | None) -> tuple[str, str]:
    versions = load_versions()
    unique_versions = sorted(set(versions.values()))
    if len(unique_versions) != 1:
        joined = ", ".join(f"{crate}={version}" for crate, version in versions.items())
        raise ReleaseError(f"release crate versions are out of sync: {joined}")

    workspace_version = unique_versions[0]
    if requested_version is None:
        return workspace_version, f"v{workspace_version}"

    normalized_version = normalize_version(requested_version)
    if normalized_version != workspace_version:
        raise ReleaseError(
            f"requested version `{normalized_version}` does not match crate version "
            f"`{workspace_version}`"
        )
    return normalized_version, f"v{normalized_version}"


def create_tag(tag: str, dry_run: bool) -> None:
    if dry_run:
        print(f"[dry-run] would create annotated tag `{tag}`")
        return
    git("tag", "-a", tag, "-m", f"Release {tag}")
    print(f"Created annotated tag `{tag}`")


def push_tag(tag: str, remote: str, dry_run: bool) -> None:
    if dry_run:
        print(f"[dry-run] would push `{tag}` to `{remote}`")
        return
    git("push", remote, tag)
    print(f"Pushed `{tag}` to `{remote}`")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Create and push a release tag.")
    parser.add_argument("--version", help="Release version, with or without the leading v")
    parser.add_argument("--remote", default="origin", help="Git remote to push to")
    parser.add_argument(
        "--branch",
        default="main",
        help="Branch that must own the release commit (default: main)",
    )
    parser.add_argument(
        "--allow-dirty",
        action="store_true",
        help="Skip the clean working tree check",
    )
    parser.add_argument(
        "--allow-non-main",
        action="store_true",
        help="Skip the branch name check",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print what would happen without creating/pushing the tag",
    )
    parser.add_argument(
        "--skip-remote-check",
        action="store_true",
        help="Skip remote sync/tag checks (useful for offline previews only)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    try:
        git("remote", "get-url", args.remote)
        version, tag = resolve_version(args.version)

        if not args.allow_dirty:
            ensure_clean_worktree()
        if not args.allow_non_main:
            ensure_branch(args.branch)

        if args.skip_remote_check:
            try:
                git("rev-parse", "--verify", f"refs/tags/{tag}")
            except ReleaseError:
                pass
            else:
                raise ReleaseError(f"tag `{tag}` already exists locally")
        else:
            ensure_remote_sync(args.remote, args.branch)
            ensure_tag_is_free(tag, args.remote)

        previous_tag = latest_release_tag()
        commit_count = commit_count_since(previous_tag)

        print(f"Release version: {version}")
        print(f"Release tag: {tag}")
        print(f"Remote: {args.remote}")
        print(f"Remote checks: {'skipped' if args.skip_remote_check else 'enabled'}")
        print(f"Previous release: {previous_tag or 'none'}")
        print(f"Commits since previous release: {commit_count}")

        create_tag(tag, args.dry_run)
        push_tag(tag, args.remote, args.dry_run)

        if args.dry_run:
            print("[dry-run] GitHub Actions would publish the release after the tag push")
        else:
            print("GitHub Actions will build artifacts and publish the GitHub Release.")
        return 0
    except ReleaseError as exc:
        print(f"[release] {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
