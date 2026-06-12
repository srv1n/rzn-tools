# GitHub — Design Spec

Status: Draft (Phase 1)

## Overview

Read GitHub issues/PRs/discussions, code search, and PR diffs/comments to power engineering assistants.

## Key Use Cases

- Summarize a PR with context from linked issues and reviews.
- Search issues by label/repo and prioritize.
- Answer questions about a code path using code search + file fetch.

## MVP Scope (Tools)

- `list_issues`: by repo/org with filters (state, labels, assignee).
- `get_issue`: issue + comments + events.
- `list_pull_requests`: by repo with filters (state, author, label).
- `get_pull_request`: PR details + reviews + comments + requested reviewers.
- `get_pull_diff`: unified diff (size‑guarded) for summarization.
- `code_search`: repo or org scoped.
- `get_file`: fetch by path/ref (guard large/binary).
- `test_auth`.

## API & Auth

- Auth: fine‑grained PAT recommended (repo read, metadata, code read).
- REST v3 endpoints; GraphQL v4 optional for efficient traversals.
- Pagination: `Link` headers; GraphQL cursors if used.

## Rust Crates / Deps

- Preferred: `octocrab` (GitHub REST/GraphQL client).
- Fallback: `reqwest`.

## Data Model

- `Issue`, `Comment`, `PullRequest`, `Review`, `DiffChunk`, `CodeSearchResult` with provenance (repo, owner, number, sha, html_url).

## Error Handling & Limits

- Respect secondary rate limits; handle `403` with `X-RateLimit-Remaining/Reset`.

## Security & Privacy

- Never log private repo names unless user opts in; redact titles in info logs for private repos.

## Local vs Server

- Server‑side; local vscode helper lives in a separate spec.

## Testing Plan

- Fixtures for issues/PRs/diffs; acceptance: summarize PR  with linked issue context.

## Implementation Checklist

- [ ] PAT auth + `test_auth`
- [ ] Issues/PRs/read APIs + code search
- [ ] Diff fetch with size checks
- [ ] Rate limit handling
- [ ] Docs and examples

