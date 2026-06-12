# Secrets (1Password, Bitwarden, HashiCorp Vault) — Design Spec

Status: Draft (Phase 1)

## Overview

Fetch secrets on behalf of tools/agents with zero retention. Prefer local CLIs for desktop and official APIs for server deployments.

## Key Use Cases

- Retrieve API keys/credentials by title/UUID for transient use in downstream calls.
- Never print or persist secret values; return handles where possible.

## MVP Scope (Tools)

- `get_secret`: by name/uuid; backend‑agnostic.
- `list_secrets`: optional, returns metadata (never values).
- `test_auth` per backend.

## Backends & Auth

- 1Password
  - Desktop: `op` CLI (session; `op item get --format json`).
  - Server: 1Password Connect server (token + REST).
- Bitwarden
  - Desktop: `bw` CLI (`bw get item`), unlocked session required.
  - Server: Bitwarden Secrets Manager API where available, or CLI via runner.
- HashiCorp Vault
  - Server: `vaultrs` crate or REST; token, approle, or JWT auth.

## Rust Crates / Deps

- CLI integration: spawn processes via `tokio::process` and parse JSON.
- Vault: `vaultrs` (or REST with `reqwest`).
- Common: `secrecy` for secret types; ensure logs redact.

## Data Model

- `SecretMaterial` (id/name, type, created/updated, value? [redacted], provider, vault/path).

## Error Handling & Limits

- Timeouts for CLI/API; descriptive errors for locked vaults; never include values in errors.

## Security & Privacy

- Zero logging of values; in‑memory only; optional process‑level redaction hooks.

## Local vs Server

- Desktop favors CLI; Server favors Connect/REST.

## Testing Plan

- Local tests against demo vault items; acceptance: resolve a secret and use it to call a test API without persisting the value.

## Implementation Checklist

- [ ] Backend selection + config
- [ ] `get_secret` + `list_secrets` + `test_auth`
- [ ] Redaction + timeouts
- [ ] Docs and examples

