# RZN DataSourcer — Next Connectors Roadmap

This roadmap proposes the next set of high‑value connectors for both server and desktop agents, plus per‑connector design specs found alongside this file. It builds on the existing connectors in `rzn_tools_core/src/connectors` (Google Workspace, Microsoft Graph scaffold, YouTube, Reddit, Wikipedia, Web, etc.).

## Design Principles (Tools for Agents)

We follow a “tools for agents” approach inspired by Anthropic’s guidance on writing tools for agents. At a glance:

- Keep tools small, composable, and single‑purpose (one clear action per tool).
- Prefer deterministic, idempotent operations with explicit inputs/outputs (JSON Schema).
- Make side‑effects opt‑in (dry‑run/confirm patterns) and safe by default.
- Return structured results plus concise `text` for LLM summarization.
- Provide typed errors, timeouts, and backoff/rate‑limit handling.
- Minimize scopes/permissions; default to read‑only; surface provenance/ACLs.
- Include a `test_auth` tool and smoke‑test paths for fast validation.

See `TOOLING_GUIDELINES.md` for concrete conventions we’ll use across connectors.

## Prioritized Build Plan (90 days)

Phase 1 — Server (highest ROI)

1. Slack
2. Microsoft Graph (Teams + OneDrive/SharePoint)
3. GitHub
4. Atlassian (Jira + Confluence)
5. Enterprise File Sync (Box + Dropbox)
6. Object Storage (S3 + GCS + Azure Blob)
7. Data Warehouses (Snowflake + BigQuery)
8. Directory Providers (Okta + Azure AD via Graph)
9. Secrets (1Password, Bitwarden, Vault)
10. Compliance (Vanta, Drata)
11. Tailscale (device inventory + status)

Phase 1 — Desktop/Local

1. Browsers (Chrome, Safari, Edge, Arc — tabs/history/bookmarks)
2. Local File System Indexer
3. Apple PIM (Notes/Reminders/Calendar via AppleScript/JXA)
4. Local Secrets (1Password CLI, Bitwarden CLI)
5. VS Code Workspace
6. Notion Desktop Shortcuts
7. OS Automation (Windows PowerShell, Linux DBus/Portals)

Phase 2 (stubs in this roadmap; specs to follow)

- Zoom/Google Meet, Linear/Asana/ClickUp, PagerDuty/Datadog/Sentry, Salesforce/Zendesk expansions, Expense/Finance (Concur, Ramp, Brex, Zip), Discord, plus additional enterprise SaaS demanded by customers.

## Contents

- TOOLING_GUIDELINES.md — cross‑connector conventions (schemas, errors, auth, logging)
- TEMPLATE.md — skeleton used for each connector spec

Server‑side connector specs

- slack.md
- linkedin.md
- microsoft_graph.md
- github.md
- atlassian.md
- enterprise_file_sync.md
- object_storage.md
- data_warehouses.md
- directory_providers.md
- secrets.md
- compliance.md
- tailscale.md

Desktop/local connector specs

- browsers.md
- local_fs.md
- apple_pim.md
- vscode.md
- os_automation.md

## Notes

- Each spec contains: MVP tool list, API endpoints, Rust crates/deps, auth scopes, data model, rate‑limits, security/PII posture, testing plan, and an implementation checklist.
- When a mature Rust SDK is unavailable, we standardize on `reqwest` + OpenAPI docs and add lightweight client modules inside `rzn_tools_core`.
