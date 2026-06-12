# Compliance (Vanta + Drata) — Design Spec

Status: Draft (Phase 1)

## Overview

Read controls, tests/evidence, and status from compliance platforms to inform agents about control posture and gaps.

## Key Use Cases

- List open control failures and assigned owners.
- Retrieve evidence links and due dates.

## MVP Scope (Tools)

- `list_controls`: filter by standard/status.
- `get_control`: details + evidence links.
- `list_tasks`: open actions with owners/due dates.
- `test_auth`.

## API & Auth

- Vanta/Drata: REST APIs with API tokens; exact endpoints vary by plan.

## Rust Crates / Deps

- `reqwest`, `serde`, `chrono`.

## Data Model

- `Control` (id, name, standard, status, owner, evidence[]).
- `Task` (id, title, status, due, owner).

## Error Handling & Limits

- Normalize statuses; backoff on 429; handle plan‑based feature flags.

## Security & Privacy

- Treat evidence URLs as sensitive; never fetch evidence content by default.

## Local vs Server

- Server‑side only.

## Testing Plan

- Recorded fixtures; acceptance: list failing controls for SOC2 with owners.

## Implementation Checklist

- [ ] Auth + `test_auth`
- [ ] Controls/tasks list/get
- [ ] Pagination + errors
- [ ] Docs and examples

