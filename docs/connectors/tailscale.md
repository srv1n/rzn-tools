# Tailscale — Design Spec

Status: Draft (Phase 1)

## Overview

Read device inventory, routes, and status to help agents understand network topology and connectivity.

## Key Use Cases

- List devices with IPs, tags, and last seen times.
- Show which devices advertise subnets or act as exit nodes.

## MVP Scope (Tools)

- `list_devices`: inventory with filters.
- `get_device`: details and current status.
- `test_auth`.

## API & Auth

- Admin API: API key; endpoints for devices and tailnet info.
- Local CLI (desktop): `tailscale status --json` as a fallback.

## Rust Crates / Deps

- REST via `reqwest`; local CLI via `tokio::process`.

## Data Model

- `TailnetDevice` (id, name, addresses[], os, user, tags[], lastSeen, online, routes[]).

## Error Handling & Limits

- Handle 429; sanitize IPs if configured; treat tailnet name as sensitive.

## Security & Privacy

- Do not log private IPs or device names at info level.

## Local vs Server

- Server uses Admin API; desktop can use local CLI.

## Testing Plan

- Fixtures for small tailnet; acceptance: list devices and flag online/offline correctly.

## Implementation Checklist

- [ ] Auth + `test_auth`
- [ ] List/get devices
- [ ] Errors + privacy
- [ ] Docs and examples

