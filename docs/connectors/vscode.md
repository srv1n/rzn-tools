# VS Code Workspace — Design Spec

Status: Draft (Phase 1)

## Overview

Local helper to surface recent workspaces, open files, and code search within current repo/workspace.

## Key Use Cases

- List recently opened folders and workspaces.
- Search filenames and code snippets quickly.

## MVP Scope (Tools)

- `list_recent_workspaces`.
- `search_workspace_files`: glob + substring/regex.
- `get_file`: guarded local read.

## Implementation

- Read VS Code recent list (platform‑specific JSON/SQLite); fall back to file system scans.
- Reuse local FS reader for content.

## Rust Crates / Deps

- `serde_json`, `rusqlite` (if needed), `ignore`.

## Security & Privacy

- Local‑only; respect ignore rules; never read outside workspace without opt‑in.

## Testing Plan

- Fixture files; acceptance: list a known workspace and find a file by name.

## Implementation Checklist

- [ ] Parse recent workspaces
- [ ] Search helper wired
- [ ] Docs and examples

