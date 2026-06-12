# Salesforce — Design Spec

Status: Draft (Phase 1)

## Overview

Read‑only access to core CRM objects (Cases, Opportunities, Accounts, Contacts) and search to power sales/support intelligence.

## Key Use Cases

- Pull a Case with full comment/activity history.
- Search Opportunities by stage/owner and summarize pipeline.

## MVP Scope (Tools)

- `soql_query`: parameterized SOQL against an allow‑listed set of objects/fields.
- `sosl_search`: free‑text search with object filters.
- `get_record`: by object + id (expand relationships minimally).
- `list_recent`: by object type with time window.
- `test_auth`.

## API & Auth

- OAuth 2.0 (Web Server or JWT Bearer for server); instance URL + REST vXX.
- Endpoints: `/services/data/vXX.X/query`, `/search`, `/sobjects/{object}/{id}`.
- Pagination: `nextRecordsUrl` for SOQL; normalize to `next_cursor`.

## Rust Crates / Deps

- `reqwest`, `oauth2`, `serde`.

## Data Model

- `SObject` (objectName, id, fields map) with provenance (`instanceUrl`, `apiVersion`).

## Error Handling & Limits

- Respect daily API quotas; backoff on 403 with limit messages; surface `errorCode`.

## Security & Privacy

- Allow‑list objects/fields; redact PII in logs.

## Testing Plan

- Fixtures for SOQL query and record fetch; acceptance: retrieve a Case and summarize last 5 activities.

## Implementation Checklist

- [ ] OAuth + `test_auth`
- [ ] SOQL/SOSL + get/list
- [ ] Pagination + quotas
- [ ] Docs and examples

