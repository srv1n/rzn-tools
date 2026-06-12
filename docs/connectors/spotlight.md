# macOS Spotlight — Design Spec

Status: Implemented (Phase 1)

## Overview

macOS-native file search connector using Spotlight (via `mdfind` CLI). Provides programmatic access to search all Spotlight-indexed content including documents, source code, emails, images, and more. Returns file paths and metadata without reading file contents.

**Platform:** macOS only (uses `mdfind` and `mdls` CLIs)

## Key Use Cases

- Search local files by content: "Find all documents mentioning 'quarterly report'"
- Search by filename: "Find files named 'config.yaml'"
- Search by file type: "Find all PDFs in Documents folder"
- Find recently modified files: "What files did I edit today?"
- Get file metadata: "What is the content type of this file?"
- Raw Spotlight queries for power users

## MVP Scope (Tools)

### `search_content`
Full-text search across all Spotlight-indexed files.
- **Inputs:** `query` (required), `directory` (optional), `kind` (optional), `limit` (default: 50)
- **Output:** `{ count, files: [paths], spotlight_query }`

### `search_by_name`
Fast filename search with partial matching.
- **Inputs:** `name` (required), `directory` (optional), `limit` (default: 50)
- **Output:** `{ count, files: [paths] }`

### `search_by_kind`
Find files by type/category.
- **Inputs:** `kind` (required, enum: pdf, image, video, audio, document, email, code, text, markdown, spreadsheet, presentation, application, folder), `directory` (optional), `limit` (default: 50)
- **Output:** `{ count, files: [paths], spotlight_query }`

### `search_recent`
Find recently modified files.
- **Inputs:** `days` (default: 7), `kind` (optional), `directory` (optional), `limit` (default: 50)
- **Output:** `{ count, files: [paths], days, spotlight_query }`

### `get_metadata`
Get Spotlight metadata for a specific file.
- **Inputs:** `path` (required)
- **Output:** `{ path, metadata: { kMDItem* attributes } }`

### `raw_query`
Execute raw mdfind queries for advanced users.
- **Inputs:** `query` (required), `directory` (optional), `limit` (default: 50)
- **Output:** `{ count, files: [paths], query }`

## CLI / API

Uses macOS system utilities:
- `mdfind` — Spotlight search CLI
- `mdls` — Get metadata for files

No external API calls or authentication required.

## Rust Crates / Deps

- `tokio::process::Command` — Async process execution
- No additional dependencies (uses system CLIs)

## Data Model

Returns file paths as strings and metadata as key-value JSON objects.

**Supported file kinds:**
| Kind | Spotlight Query |
|------|----------------|
| `pdf` | `kMDItemContentType == "com.adobe.pdf"` |
| `image` | `kMDItemContentTypeTree == "public.image"` |
| `video` | `kMDItemContentTypeTree == "public.movie"` |
| `audio` | `kMDItemContentTypeTree == "public.audio"` |
| `document` | `kMDItemContentTypeTree == "public.content"` |
| `email` | `kMDItemContentType == "com.apple.mail.emlx"` |
| `code` | `kMDItemContentTypeTree == "public.source-code"` |
| `text` | `kMDItemContentTypeTree == "public.plain-text"` |
| `markdown` | `kMDItemContentType == "net.daringfireball.markdown"` |
| `folder` | `kMDItemContentType == "public.folder"` |
| `application` | `kMDItemContentType == "com.apple.application-bundle"` |

## Error Handling & Limits

- Returns empty array if no results found
- Default limit of 50 results per query
- CLI errors propagated as `ConnectorError::Other`
- Non-macOS platforms return clear error message

## Security & Privacy

- **Local-only:** No data leaves the machine
- **Read-only:** Only searches and reads metadata; cannot modify files
- **No file content access:** Returns paths only; users must explicitly read files
- **Respects Spotlight privacy:** Folders excluded from Spotlight are not searched

## Local vs Server

- **Local only:** This connector only works on macOS
- **No network:** All operations are local filesystem queries
- **Feature-gated:** Behind `macos-spotlight` feature flag

## Smart Resolver Patterns

The connector responds to these input patterns via `rzn-tools fetch`:

| Pattern | Tool | Example |
|---------|------|---------|
| `file:///path` | `get_metadata` | `rzn-tools fetch file:///Users/me/doc.pdf` |
| `/Users/...`, `/Volumes/...` | `get_metadata` | `rzn-tools fetch /Users/me/doc.pdf` |
| `~/path` | `get_metadata` | `rzn-tools fetch ~/Documents/notes.md` |
| `spotlight:query` | `search_content` | `rzn-tools fetch "spotlight:machine learning"` |
| `mdfind:query` | `search_content` | `rzn-tools fetch "mdfind:kMDItemKind == PDF"` |

## Usage Examples

```bash
# Search file contents
rzn-tools spotlight search-content --query "quarterly report"

# Search in specific directory
rzn-tools spotlight search-content --query "TODO" --directory ~/Projects

# Find files by name
rzn-tools spotlight search-by-name --name "config.yaml"

# Find all PDFs
rzn-tools spotlight search-by-kind --kind pdf

# Find recently modified code files
rzn-tools spotlight search-recent --days 3 --kind code

# Get file metadata
rzn-tools spotlight metadata --path /Users/me/report.pdf

# Using smart resolver
rzn-tools fetch ~/Documents/report.pdf
rzn-tools fetch "spotlight:CRISPR gene therapy"

# Use --help for all options
rzn-tools spotlight --help
rzn-tools spotlight search-content --help
```

## Testing Plan

- **Unit:** Pattern building and query construction
- **Integration:** Run actual mdfind queries against known files
- **Acceptance:** Search returns expected files from test directory

## Implementation Checklist

- [x] List tools in `list_tools`
- [x] Implement `search_content` with full-text search
- [x] Implement `search_by_name` with filename matching
- [x] Implement `search_by_kind` with type filtering
- [x] Implement `search_recent` with date filtering
- [x] Implement `get_metadata` for file attributes
- [x] Implement `raw_query` for power users
- [x] Add smart resolver patterns
- [x] Platform guards (`#[cfg(target_os = "macos")]`)
- [x] Docs and examples
