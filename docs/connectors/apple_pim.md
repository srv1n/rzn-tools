# Apple PIM (Notes, Reminders, Calendar) — Design Spec

Status: Draft (Phase 1)

## Overview

Extend the macOS automation connector with AppleScript/JXA tools for Notes, Reminders, and Calendar. macOS‑only; TCC prompts expected.

## Key Use Cases

- Search notes by title/content.
- List upcoming reminders and calendar events.

## MVP Scope (Tools)

- `notes_search`: query → list of notes (title, folder, snippet, modified, note id).
- `notes_get`: id → full text (no attachments in MVP).
- `reminders_list`: upcoming/past‑due with lists and due dates.
- `calendar_list_events`: time window, calendars filter.

## Implementation

- Use existing `MacOsAutomationConnector` to run AppleScript/JXA via `/usr/bin/osascript`.
- Provide built‑in scripts; return structured JSON where possible by `JSON.stringify` in JXA.

## Rust Crates / Deps

- Existing connector + `tokio::process`; optional `plist` for metadata.

## Data Model

- `NoteItem`, `ReminderItem`, `CalendarEvent` with provenance (`account`, `calendar`, `id`).

## Error Handling & Limits

- Handle permission errors with actionable messages; timeouts for long scripts.

## Security & Privacy

- Local‑only; redact note text in logs; allow folder/calendars allow‑lists.

## Testing Plan

- Sample notes/reminders/calendars; acceptance: search and fetch a note by id.

## Implementation Checklist

- [ ] AppleScript/JXA scripts packaged
- [ ] Tools wired with schemas
- [ ] TCC guidance in docs
- [ ] Examples

