# Zendesk — Design Spec

Status: Draft (Phase 1)

## Overview

Read tickets, comments, users, and knowledge base articles to power support assistants.

## Key Use Cases

- Fetch a ticket with the conversation and attachments metadata.
- Search tickets by status, group, or requester.

## MVP Scope (Tools)

- `search_tickets`: query params → tickets.
- `get_ticket`: id → ticket + comments.
- `list_user_tickets`: by requester or assignee.
- `list_helpcenter_articles`: optional.
- `test_auth`.

## API & Auth

- Auth: OAuth 2.0 or API token (email/token basic auth).
- Endpoints: `/api/v2/search`, `/api/v2/tickets/{id}`, `/api/v2/users/{id}/tickets/*`, `/api/v2/help_center/*`.
- Pagination: `next_page` URLs.

## Rust Crates / Deps

- `reqwest`, `serde`, `chrono`.

## Data Model

- `Ticket` (id, subject, status, priority, requester, assignee, created/updated, tags, comments[] with author/time/body, attachments[] meta).

## Error Handling & Limits

- Rate limit 429 with `Retry-After`; sanitize HTML in comments; size‑guard attachments.

## Security & Privacy

- Treat ticket content as sensitive; redact emails and phone numbers in logs.

## Testing Plan

- Fixtures for search and ticket retrieval; acceptance: fetch a ticket by id with ordered conversation.

## Implementation Checklist

- [ ] Auth + `test_auth`
- [ ] Search/get/list
- [ ] Pagination + error mapping
- [ ] Docs and examples

