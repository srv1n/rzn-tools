# Microsoft Graph

Connector name: `microsoft-graph`

The current connector supports Outlook mail and calendar. It does not expose
OneDrive, Teams, or SharePoint tools.

| Tool group | Operations |
| --- | --- |
| Auth | `auth_start`, `auth_poll` |
| Mail read | `list_messages`, `get_message` |
| Calendar read | `list_events` |
| Mail write | `send_mail`, `create_draft`, attachment upload, `send_draft` |

The connector implements Microsoft device OAuth and token refresh. The config
schema also names PKCE and client-credentials modes, but this connector does
not run those flows.

The default scope list is read-oriented. Sending mail and changing drafts need
the correct Microsoft Graph write permissions. Confirm the account, recipient,
and attachment before a write.

Code: `rzn_tools_core/src/connectors/microsoft/mod.rs`

