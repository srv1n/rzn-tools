# Local File System Indexer — Design Spec

Status: Draft (Phase 1)

## Overview

Local‑only indexer and fetcher for documents under selected roots (Documents, Downloads, custom). Incremental updates via file watchers; fast search via embedded index.

## Key Use Cases

- Search personal documents by filename/content with filters.
- Fetch file content safely for RAG.

## MVP Scope (Tools)

- `index_start`: build index for configured roots with ignore rules.
- `search_files`: query with filters (ext, size, mtime).
- `get_file`: guarded read with text extraction for common formats (txt, md, pdf via existing parsers).

## Rust Crates / Deps

- `ignore` (gitignore/.ignore support), `walkdir`, `notify` (watchers), `tantivy` (index), `rayon` (optional), `mime_guess`.

## Data Model

- `FsEntry` (path, name, size, mime, mtime, tags?).

## Error Handling & Limits

- Skip unreadable dirs; size caps; extension allow‑list for content extraction.

## Security & Privacy

- Local‑only; per‑folder allow‑list; never traverse outside configured roots.

## Testing Plan

- Fixture directories; acceptance: index small tree and search returns expected paths.

## Implementation Checklist

- [ ] Config schema for roots and ignore rules
- [ ] Indexer + watcher
- [ ] Search + guarded read
- [ ] Docs and examples

