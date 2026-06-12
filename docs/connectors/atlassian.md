# Atlassian (Jira + Confluence) — Design Spec

Status: Draft (Phase 1)

## Overview

Jira for issues/sprints and Confluence for pages/spaces. Read‑only MVP enables planning and knowledge retrieval.

## Key Use Cases

- Pull a Jira issue with history and linked issues.
- Search Jira with JQL and list sprint issues.
- Fetch a Confluence page rendered content + attachments for RAG.

## MVP Scope (Tools)

- Jira
  - `search_issues`: JQL, fields, paging.
  - `get_issue`: issue + comments + changelog.
  - `list_sprint_issues`: by board/sprint.
- Confluence
  - `search_pages`: CQL query.
  - `get_page`: rendered storage or view content + attachments.
  - `list_space_pages`: basic navigation.
- `test_auth` for both products.

## API & Auth

- Jira Cloud REST v3; Confluence Cloud REST v1.
- Auth: API token + email (basic auth header) or OAuth 2.0 (3LO) for org installs.
- Pagination: offset/limit (Jira), cursor for Confluence via `_links.next`.

## Rust Crates / Deps

- HTTP: `reqwest` + `serde`.
- Optional: evaluate community crates for Jira; use REST directly if incomplete.

## Data Model

- `JiraIssue` (key, summary, status, assignee, labels, fields map, comments, changelog).
- `ConfluencePage` (id, title, space, version, content_html, attachments[], ancestors[]).

## Error Handling & Limits

- Backoff on 429; normalize vendor errors; cap HTML size and sanitize.

## Security & Privacy

- Include project/space ACL hints; do not log restricted titles or URLs.

## Local vs Server

- Server‑side only.

## Testing Plan

- Fixtures for JQL search and Confluence page render; acceptance: retrieve a page with attachments and safe text conversion.

## Implementation Checklist

- [ ] Auth helpers (API token + OAuth)
- [ ] Jira search/get and Confluence search/get
- [ ] Pagination + error mapping
- [ ] Docs and examples

