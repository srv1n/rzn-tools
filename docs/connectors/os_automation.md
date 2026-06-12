# OS Automation (Windows PowerShell + Linux DBus/Portals) — Design Spec

Status: Draft (Phase 1)

## Overview

Cross‑platform system helpers for local agents: run PowerShell commands on Windows and interact with desktop portals/DBus on Linux (notifications, clipboard, open URL/file pickers).

## Key Use Cases

- Execute a safe, parameterized PowerShell script and return JSON.
- Show a desktop notification or open a URL via the default browser.

## MVP Scope (Tools)

- Windows
  - `powershell_run`: `pwsh`/`powershell.exe` with a sandboxed, parameterized script; `timeout_ms`.
- Linux
  - `portal_open_url`: xdg‑portal request to open URLs.
  - `notify_send`: DBus notification.

## Rust Crates / Deps

- Windows: spawn via `tokio::process`.
- Linux: `zbus` for DBus; `ashpd` for xdg‑desktop‑portal.

## Security & Privacy

- Deny raw shell unless explicitly enabled; parameterize scripts; scrub env vars in logs.

## Testing Plan

- Acceptance: PowerShell returns structured JSON; Linux portal opens a URL and returns success.

## Implementation Checklist

- [ ] Platform detection + helpers
- [ ] Tools and schemas
- [ ] Docs and examples

