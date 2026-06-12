# Microsoft Graph (Teams, OneDrive, SharePoint) — Design Spec

Status: Draft (Phase 1)

## Overview

Extend the existing Microsoft Graph connector beyond Mail/Calendar to Teams chat/threads and OneDrive/SharePoint file search + fetch. Device‑code and client‑credential flows supported.

## Key Use Cases

- Pull recent Teams threads and meeting chats.
- Search OneDrive/SharePoint for documents and fetch content with metadata/ACL.
- List upcoming events and related files/links.

## MVP Scope (Tools)

- `list_teams_chats`: recent chats/threads for the signed‑in user.
- `get_teams_thread`: messages for a chat/thread id.
- `search_drive`: query across user’s OneDrive and shared libraries (SharePoint) with filters.
- `get_file_metadata`: by driveItem id or path.
- `download_file`: stream with size/MIME guards.
- `list_events`: extend existing tool with attachments/onlineMeeting link.
- `test_auth`: verify token and basic profile.

## API & Auth

- Auth: Azure Entra ID; delegated (device code) and application (client credentials) where allowed.
- Scopes (delegated MVP): `Chat.Read`, `Chat.ReadBasic`, `Files.Read.All`, `Sites.Read.All`, `User.Read`, `Calendars.Read`.
- Graph endpoints: `/me/chats`, `/chats/{id}/messages`, `/me/drive/search(q='{query}')`, `/sites/{site-id}/drive/root:/path:`, `/me/events`.
- Pagination: `@odata.nextLink`; delta queries for incremental sync (follow‑up).

## Rust Crates / Deps

- Use existing `graph-rs-sdk` already referenced in the repo.
- HTTP fallback: `reqwest` with Graph REST if specific endpoints lacking SDK coverage.

## Data Model

- `TeamsMessage` (id, createdDateTime, from, body.text/html, attachments[], replies[], chatId, team/channel if available).
- `DriveItem` (id, name, size, mimeType, webUrl, lastModifiedDateTime, createdBy, share info).
- Include `siteId`/`driveId` and sharing links for provenance and ACL.

## Error Handling & Limits

- Respect throttling (`429`, `Retry-After`); map Graph `error.code`/`message` to typed errors.
- Stream downloads with byte limits; support range requests resume.

## Security & Privacy

- Store tokens encrypted; minimize scopes; default to read‑only.
- Redact PII in logs; never print message/file bodies.

## Local vs Server

- Server‑side only for Graph.

## Testing Plan

- Sandbox tenant fixtures for chats and drive search.
- Acceptance: search file by name and fetch content; fetch Teams thread by id with correct ordering.

## Implementation Checklist

- [ ] Expand scopes and device‑code flow prompts
- [ ] `list_teams_chats`, `get_teams_thread`, `search_drive`, `get_file_metadata`, `download_file`
- [ ] Pagination + throttling handling
- [ ] Docs and examples

