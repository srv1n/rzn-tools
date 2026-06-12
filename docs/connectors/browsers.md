# Browsers (Chrome, Safari, Edge, Arc) — Design Spec

Status: Draft (Phase 1)

## Overview

Local‑only connector that reads tabs, history, and bookmarks from installed browsers to power personal search and context retrieval. No network calls; zero exfiltration.

## Key Use Cases

- Search recent history for a topic and return visited pages.
- List open tabs per window/profile for quick context.
- Fetch bookmark metadata for reference sets.

## MVP Scope (Tools)

- `list_open_tabs`: browser/profile → tabs (title, URL, lastActive, window).
- `search_history`: query/time window → URLs with visit counts.
- `list_bookmarks`: folder tree to flat list or hierarchical.

## Data Sources (Paths)

- Chrome/Edge/Arc: SQLite DBs under `~/Library/Application Support/<Browser>/<Profile>/` on macOS; platform‑specific paths for Win/Linux.
- Safari: `~/Library/Safari/History.db` and Bookmarks plist.
- Implementation reads by copying DBs to a temp file to avoid lock issues.

## Rust Crates / Deps

- `rusqlite` for history DBs; `plist` for Safari bookmarks; `serde`.
- `walkdir` for profile discovery; `dirs` for cross‑platform paths.

## Data Model

- `BrowserTab` (title, url, windowId, profile, lastActive).
- `HistoryEntry` (url, title, lastVisit, visitCount, profile).
- `Bookmark` (title, url, folderPath, profile).

## Error Handling & Limits

- Handle locked DBs by copying; guard path traversal; skip corrupt profiles gracefully.

## Security & Privacy

- Local‑only; never logs URLs unless user opts in; respect per‑profile allow/deny lists.

## Local vs Server

- Desktop only.

## Testing Plan

- Unit tests against small fixture DBs; acceptance: list tabs and search recent history by keyword.

## Implementation Checklist

- [ ] Profile discovery + path handling
- [ ] Tabs/history/bookmarks readers
- [ ] Privacy filters + redaction
- [ ] Docs and examples

