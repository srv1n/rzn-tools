# Directory Providers (Okta + Azure AD) — Design Spec

Status: Draft (Phase 1)

## Overview

Directory read for users, groups, and basic app assignments to power mention resolution and access checks.

## Key Use Cases

- Resolve an email/handle to a display name and org group memberships.
- Enumerate group members for access filters in downstream queries.

## MVP Scope (Tools)

- `get_user`: by id/email/login.
- `list_group_members`: by group id.
- `search_users`: query string.
- `test_auth`.

## API & Auth

- Okta: API token; `/api/v1/users`, `/api/v1/groups/{id}/users`.
- Azure AD: via Microsoft Graph `/users`, `/groups/{id}/members` with `User.Read.All` and `Group.Read.All` (app permissions often required).
- Pagination: vendor cursors/links.

## Rust Crates / Deps

- Okta: REST via `reqwest`.
- Azure AD: existing `graph-rs-sdk`.

## Data Model

- `DirectoryUser` (id, displayName, emails[], manager?, groups[], avatar?).
- `DirectoryGroup` (id, displayName, members[] link).

## Error Handling & Limits

- Respect rate limits; handle deactivated users and partial profiles.

## Security & Privacy

- Treat directory data as sensitive; never log emails in info logs.

## Local vs Server

- Server‑side.

## Testing Plan

- Fixtures for user/group listings; acceptance: resolve an email to user + groups.

## Implementation Checklist

- [ ] Okta + Graph auth
- [ ] User search/get and group members
- [ ] Pagination + errors
- [ ] Docs and examples

