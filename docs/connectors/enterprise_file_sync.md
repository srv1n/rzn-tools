# Enterprise File Sync (Box + Dropbox) — Design Spec

Status: Draft (Phase 1)

## Overview

Read/search enterprise files stored in Box and Dropbox. Start read‑only with metadata‑rich listings and guarded downloads.

## Key Use Cases

- Search files/folders by name or content preview.
- Fetch a file with provenance and sharing info.
- List recently modified items for a user/team.

## MVP Scope (Tools)

- `search_files`: query with path scope and type filters.
- `list_folder`: children with pagination.
- `get_file_metadata`: id → metadata and web link.
- `download_file`: stream with size/MIME guards.
- `test_auth`.

## API & Auth

- Box: OAuth 2.0; REST v2 (`/search`, `/folders/{id}/items`, `/files/{id}/content`).
- Dropbox: OAuth 2.0; API 2 (`/files/search_v2`, `/files/list_folder`, `/files/download`).
- Pagination: per‑service cursors; normalize to `next_cursor`.

## Rust Crates / Deps

- Box: REST via `reqwest`.
- Dropbox: evaluate `dropbox-sdk` for Rust; otherwise `reqwest`.
- Common: `tokio`, `serde`, `mime_guess`.

## Data Model

- `CloudFile` (id, name, size, mime, modified, path, webUrl, owner, share/permissions).

## Error Handling & Limits

- Respect vendor 429/5xx; cap downloads; sanitize filenames; handle shared link permission errors.

## Security & Privacy

- Redact filenames from logs unless opted‑in; never log path roots of private folders.

## Local vs Server

- Server‑side.

## Testing Plan

- Fixtures for search/list/download; acceptance: find a known doc and fetch within size bounds.

## Implementation Checklist

- [ ] OAuth + `test_auth`
- [ ] Search/list/get/download
- [ ] Pagination + errors
- [ ] Docs and examples

