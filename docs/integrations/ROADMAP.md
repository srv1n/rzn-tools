# Integrations roadmap

This page lists work that is still missing. It is not a description of current
connectors. See [../system/connectors.md](../system/connectors.md) for current
behavior.

Implemented foundations include Microsoft Graph mail/calendar, Google Drive,
Gmail, Calendar, People, GitHub, Atlassian, Slack, IMAP, SMTP, CalDAV, and the
Apple personal-data connectors.

Known product gaps include:

- Microsoft Graph OneDrive, Teams, and SharePoint tools
- Notion, Dropbox, and Box connectors
- consistent write-scope checks for Google and Microsoft tools
- dedicated current pages for every implemented connector
- normalized output for more indexable tools
- federated global timeout and deduplication behavior

Add roadmap items only after code and existing connectors have been checked.
Do not name a dependency before the implementation needs it.
