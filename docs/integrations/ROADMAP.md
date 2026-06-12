# Integrations Roadmap

This repo aims to reach feature parity with popular “OpenAI connectors” by adding well-supported, feature‑gated adapters.

Phased plan and selected crates:

- Phase 1
  - Microsoft 365 via Microsoft Graph → `graph-rs-sdk`
    - Coverage: Outlook Mail/Calendar, SharePoint/OneDrive, Teams
  - Google Drive → `google-drive3` (+ `yup-oauth2`)

- Phase 2
  - Gmail → `google-gmail1`
  - Google Calendar → `google-calendar3`
  - Google Contacts (People API) → `google-people1`

- Phase 3
  - Notion → `notion-client`

- Phase 4
  - Dropbox → `dropbox-sdk-rust`

- Phase 5
  - Box → REST/OpenAPI codegen (no mature Rust SDK)

Prioritization factors:
- Maintenance and release cadence of crates
- API coverage and auth story (OAuth flows, refresh tokens)
- Docs and examples quality
- Rate limit/backoff + pagination/changes support

Next up (Phase 1):
- Ship minimal, end‑to‑end “list” operations for Graph and Drive
- Land OAuth scaffolding and token persistence
- Add examples and integration tests behind feature flags

