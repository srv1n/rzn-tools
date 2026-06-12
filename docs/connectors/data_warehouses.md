# Data Warehouses (Snowflake + BigQuery) — Design Spec

Status: Draft (Phase 1)

## Overview

Read‑only SQL query execution against Snowflake and BigQuery to answer analytical questions with strong guardrails.

## Key Use Cases

- Run parameterized queries against approved schemas.
- Return small, typed result sets suitable for LLMs.

## MVP Scope (Tools)

- `execute_query`: SQL with named parameters (server‑side allow‑list). Returns rows + schema.
- `list_tables`: database/schema → tables/views.
- `get_table_schema`: columns + types + comments.
- `test_auth`.

## API & Auth

- Snowflake: use ODBC or REST API; auth via key pair or username/password + MFA (prefer key pair for service).
- BigQuery: service account; REST jobs or query endpoint; per‑project dataset scopes.
- Pagination: chunk result sets; max rows limit with truncation indicators.

## Rust Crates / Deps

- Snowflake: evaluate `odbc-api` crate or vendor REST via `reqwest`.
- BigQuery: `google-apis-rs` BigQuery client or REST via `reqwest`.
- Common: `serde_json`, `arrow` (optional) for typed data interchange.

## Data Model

- `QueryResult` (columns[], rows[], rowCount, truncated, stats: elapsedMs, bytesProcessed?).

## Error Handling & Limits

- Timeout queries; enforce allow‑listed schemas; reject DML/DDL; parameterize to avoid injection.

## Security & Privacy

- Do not log SQL text by default; redact identifiers; audit provenance (project/dataset/table, statement id).

## Local vs Server

- Server‑side only.

## Testing Plan

- Small public datasets; acceptance: run parameterized query within row limits.

## Implementation Checklist

- [ ] Auth + `test_auth`
- [ ] Execute/list/schema with limits
- [ ] Timeouts + allow‑lists
- [ ] Docs and examples

