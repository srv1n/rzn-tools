# Setup: Microsoft Graph and Google Drive

This document captures minimal steps to get local OAuth working for Phase 1 connectors.

Microsoft Graph (Azure Entra ID)
- Register an app in Azure Portal → App registrations.
- Authentication:
  - For local delegated flows: enable `http://localhost:PORT` (or device code).
  - For app-only flows: create a client secret.
- API permissions (incremental):
  - Mail: `Mail.Read` (delegated) or `Mail.Read` (application + admin consent)
  - Calendar: `Calendars.Read`
  - Files (OneDrive/SharePoint): `Files.Read.All`
- Env/Config keys used by the connector:
  - `GRAPH_TENANT_ID`, `GRAPH_CLIENT_ID`, `GRAPH_CLIENT_SECRET` (if app creds)
  - Scopes: space‑separated (e.g. `Mail.Read Calendars.Read Files.Read.All`)

Google Drive (Google Cloud Console)
- Create OAuth client credentials (Desktop app or Web app for PKCE).
- Add redirect URI for local dev (e.g. `http://localhost:PORT/callback`).
- Enable “Google Drive API” for the project.
- Scopes:
  - Read-only: `https://www.googleapis.com/auth/drive.readonly`
  - Gmail: `https://www.googleapis.com/auth/gmail.readonly`
  - Calendar: `https://www.googleapis.com/auth/calendar.readonly`
  - People (Contacts): `https://www.googleapis.com/auth/contacts.readonly`
- Env/Config keys used by the connector:
  - `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_REDIRECT_URI`

Local dev notes
- The connectors are feature‑gated; enable via Cargo features, examples:
  - `--features "microsoft-graph,google-drive,google-gmail,google-calendar,google-people"`
- Tokens persist to `~/.config/rzn-tools/auth.json` by default.
- Device flow
  - Start: call the connector tool `auth_start` to get `user_code` and a URL
  - Complete: open the URL, enter the code
  - Poll: call `auth_poll` with `device_code` (saves tokens automatically)
