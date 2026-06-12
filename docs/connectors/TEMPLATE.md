# <Connector Name> — Design Spec

Status: Draft

## Overview

Short description and the primary value this connector unlocks.

## Key Use Cases

- Use case 1
- Use case 2

## MVP Scope (Tools)

- `tool_name`: one‑line description
  - Inputs: {...}
  - Output: {...}

## API & Auth

- Endpoints to call first, auth method/scopes, rate limits, and pagination model.

## Rust Crates / Deps

- `reqwest`, `serde`, `oauth2`, etc. and any vendor SDK if mature.

## Data Model

- Resource types returned; fields we normalize; provenance & ACLs.

## Error Handling & Limits

- Error taxonomy, retries, and backoff.

## Security & Privacy

- PII handling, scope minimization, local‑only mode.

## Local vs Server

- When/how this runs locally vs in server; caching/sync.

## Testing Plan

- Unit, integration, and acceptance criteria.

## Implementation Checklist

- [ ] List tools in `list_tools`
- [ ] Implement calls with schemas
- [ ] Auth + `test_auth`
- [ ] Pagination, errors, timeouts
- [ ] Docs and examples

