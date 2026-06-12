#!/usr/bin/env python3
"""
Generate GitHub release notes from the delta since the previous release tag.

If OPENAI_API_KEY is present, this script uses the OpenAI Responses API to turn the
commit/changelog diff into a concise public release summary. Without that secret, it
falls back to a deterministic markdown summary so release publication still works.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO_SLUG = "srv1n/rzn-tools"
DEFAULT_MODEL = "gpt-5.2"


class NotesError(RuntimeError):
    """Raised when release note generation cannot proceed."""


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
        raise NotesError(f"{' '.join(args)}: {detail}")
    return proc.stdout.strip()


def git(*args: str) -> str:
    return run("git", *args)


def semver_tags() -> list[str]:
    return [
        tag.strip()
        for tag in git("tag", "--list", "v*", "--sort=-v:refname").splitlines()
        if tag.strip()
    ]


def resolve_previous_tag(current_tag: str) -> str | None:
    tags = semver_tags()
    if current_tag not in tags:
        raise NotesError(
            f"tag `{current_tag}` is not present locally; fetch tags before generating notes"
        )
    index = tags.index(current_tag)
    return tags[index + 1] if index + 1 < len(tags) else None


def commit_range(previous_tag: str | None, current_tag: str) -> str:
    return f"{previous_tag}..{current_tag}" if previous_tag else current_tag


def commit_range_for_ref(previous_tag: str | None, ref: str) -> str:
    return f"{previous_tag}..{ref}" if previous_tag else ref


def collect_commits(range_spec: str) -> list[dict[str, str]]:
    raw = git("log", "--reverse", "--format=%h%x1f%s%x1f%an", range_spec)
    commits: list[dict[str, str]] = []
    for line in raw.splitlines():
        if not line.strip():
            continue
        sha, subject, author = line.split("\x1f")
        commits.append({"sha": sha, "subject": subject, "author": author})
    return commits


def collect_changed_files(previous_tag: str | None, current_tag: str) -> list[str]:
    if previous_tag is None:
        raw = git("ls-tree", "-r", "--name-only", current_tag)
    else:
        raw = git("diff", "--name-only", f"{previous_tag}..{current_tag}")
    return [line.strip() for line in raw.splitlines() if line.strip()]


def collect_changed_files_for_ref(previous_tag: str | None, ref: str) -> list[str]:
    if previous_tag is None:
        raw = git("ls-tree", "-r", "--name-only", ref)
    else:
        raw = git("diff", "--name-only", f"{previous_tag}..{ref}")
    return [line.strip() for line in raw.splitlines() if line.strip()]


def extract_unreleased_changelog() -> str:
    changelog = ROOT / "CHANGELOG.md"
    if not changelog.exists():
        return ""

    lines = changelog.read_text(encoding="utf-8").splitlines()
    capture = False
    collected: list[str] = []

    for line in lines:
        if line.startswith("## [Unreleased]"):
            capture = True
            continue
        if capture and line.startswith("## ["):
            break
        if capture:
            collected.append(line)

    return "\n".join(collected).strip()


def clip(text: str, *, max_chars: int) -> str:
    if len(text) <= max_chars:
        return text
    return text[: max_chars - 3].rstrip() + "..."


def llm_summary(
    *,
    current_tag: str,
    previous_tag: str | None,
    commits: list[dict[str, str]],
    changed_files: list[str],
    changelog_excerpt: str,
) -> str | None:
    api_key = os.environ.get("OPENAI_API_KEY", "").strip()
    if not api_key:
        return None

    prompt_parts = [
        f"Current release tag: {current_tag}",
        f"Previous release tag: {previous_tag or 'none'}",
        "",
        "Commits:",
    ]
    prompt_parts.extend(
        f"- {commit['sha']} {commit['subject']} ({commit['author']})" for commit in commits[:80]
    )
    prompt_parts.extend(["", "Changed files:"])
    prompt_parts.extend(f"- {path}" for path in changed_files[:120])

    if changelog_excerpt:
        prompt_parts.extend(
            [
                "",
                "CHANGELOG.md [Unreleased] excerpt:",
                clip(changelog_excerpt, max_chars=6000),
            ]
        )

    prompt = "\n".join(prompt_parts)
    model = os.environ.get("OPENAI_MODEL", DEFAULT_MODEL).strip() or DEFAULT_MODEL

    payload = {
        "model": model,
        "input": prompt,
        "instructions": (
            "Write concise public GitHub release notes for rzn-tools, a Rust CLI and MCP server. "
            "Return markdown only. Use exactly these sections: `## Highlights` and "
            "`## What's In This Release`. Keep it under 350 words. Prioritize user-visible "
            "features, release automation changes, packaging/installer changes, and new "
            "connectors. If commit subjects still use legacy names, describe the current "
            "product as `rzn-tools` instead of repeating stale branding. "
            "Do not invent features not supported by the commits or changelog excerpt."
        ),
        "max_output_tokens": 900,
    }

    request = urllib.request.Request(
        "https://api.openai.com/v1/responses",
        method="POST",
        data=json.dumps(payload).encode("utf-8"),
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": "application/json",
        },
    )

    org = os.environ.get("OPENAI_ORG_ID", "").strip()
    if org:
        request.add_header("OpenAI-Organization", org)
    project = os.environ.get("OPENAI_PROJECT_ID", "").strip()
    if project:
        request.add_header("OpenAI-Project", project)

    try:
        with urllib.request.urlopen(request, timeout=90) as response:
            raw = response.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        detail = exc.read().decode("utf-8", errors="replace")
        print(
            f"[release-notes] OpenAI summary failed, falling back to deterministic notes: "
            f"{exc.code} {detail}",
            file=sys.stderr,
        )
        return None
    except OSError as exc:
        print(
            f"[release-notes] OpenAI summary failed, falling back to deterministic notes: {exc}",
            file=sys.stderr,
        )
        return None

    value = json.loads(raw)
    summary = str(value.get("output_text", "")).strip()
    return summary or None


def fallback_summary(
    *,
    commits: list[dict[str, str]],
    changelog_excerpt: str,
) -> str:
    bullets = [
        line.strip()
        for line in changelog_excerpt.splitlines()
        if line.strip().startswith("- ")
    ]

    if bullets:
        highlight_lines = bullets[:5]
        detail_lines = bullets[5:12]
        parts = ["## Highlights"]
        parts.extend(highlight_lines)
        parts.extend(["", "## What's In This Release"])
        parts.extend(detail_lines or highlight_lines)
        return "\n".join(parts)

    commit_lines = [
        f"- {commit['subject']}"
        for commit in commits[:10]
    ]
    if not commit_lines:
        commit_lines = ["- Internal maintenance and release packaging updates."]

    parts = [
        "## Highlights",
        "- LLM-generated release summary was unavailable, so this release note is using the raw git delta.",
        "",
        "## What's In This Release",
        *commit_lines,
    ]
    return "\n".join(parts)


def build_release_body(
    *,
    current_tag: str,
    previous_tag: str | None,
    summary: str,
) -> str:
    compare_url = (
        f"https://github.com/{REPO_SLUG}/compare/{previous_tag}...{current_tag}"
        if previous_tag
        else None
    )

    parts = [
        f"## rzn-tools {current_tag}",
        "",
        summary.strip(),
        "",
        "## Install",
        "",
        "```bash",
        "curl -fsSL https://raw.githubusercontent.com/srv1n/rzn-tools/main/packaging/scripts/install.sh | bash",
        "```",
        "",
        "## Workflow Assets",
        "",
        "```bash",
        "rzn-tools workflows list",
        "rzn-tools workflows sync --remote",
        "```",
        "",
        "## Release Assets",
        "",
        f"- `rzn-tools-{current_tag}-x86_64-unknown-linux-gnu.tar.gz`",
        f"- `rzn-tools-{current_tag}-x86_64-pc-windows-msvc.zip`",
        f"- `rzn-tools-{current_tag}-x86_64-apple-darwin.tar.gz`",
        f"- `rzn-tools-{current_tag}-aarch64-apple-darwin.tar.gz`",
        f"- `rzn-tools-workflows-{current_tag}.tar.gz`",
        "- `checksums.txt`",
    ]

    if compare_url:
        parts.extend(["", f"[Full Changelog]({compare_url})"])

    return "\n".join(parts).strip() + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate GitHub release notes.")
    parser.add_argument("--tag", required=True, help="Release tag, e.g. v0.2.17")
    parser.add_argument(
        "--output",
        required=True,
        help="Path to write the generated markdown body",
    )
    parser.add_argument(
        "--previous-tag",
        help="Override the previous release tag instead of auto-detecting it",
    )
    parser.add_argument(
        "--allow-unreleased-tag",
        action="store_true",
        help="Allow previewing notes for a tag that does not exist yet by diffing from HEAD",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()

    try:
        current_tag = args.tag.strip()
        tags = semver_tags()
        tag_exists = current_tag in tags
        if args.previous_tag:
            previous_tag = args.previous_tag.strip()
        elif tag_exists:
            previous_tag = resolve_previous_tag(current_tag)
        else:
            previous_tag = tags[0] if tags else None

        if tag_exists:
            range_spec = commit_range(previous_tag, current_tag)
            commits = collect_commits(range_spec)
            changed_files = collect_changed_files(previous_tag, current_tag)
        else:
            if not args.allow_unreleased_tag:
                raise NotesError(
                    f"tag `{current_tag}` is not present locally; pass --allow-unreleased-tag "
                    "to preview notes from HEAD before tagging"
                )
            range_spec = commit_range_for_ref(previous_tag, "HEAD")
            commits = collect_commits(range_spec)
            changed_files = collect_changed_files_for_ref(previous_tag, "HEAD")

        changelog_excerpt = extract_unreleased_changelog()

        summary = llm_summary(
            current_tag=current_tag,
            previous_tag=previous_tag,
            commits=commits,
            changed_files=changed_files,
            changelog_excerpt=changelog_excerpt,
        )
        if summary is None:
            summary = fallback_summary(commits=commits, changelog_excerpt=changelog_excerpt)

        body = build_release_body(
            current_tag=current_tag,
            previous_tag=previous_tag,
            summary=summary,
        )
        output = Path(args.output)
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(body, encoding="utf-8")
        print(f"Wrote release notes to {output}")
        return 0
    except NotesError as exc:
        print(f"[release-notes] {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
