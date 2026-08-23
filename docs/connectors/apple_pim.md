# Apple personal data connectors

These connectors use macOS Automation and privacy permissions. They do not use
API-key credentials.

| Connector | Advertised operations |
| --- | --- |
| `apple-mail` | List mailboxes/messages, get/search messages, create drafts, send. |
| `apple-contacts` | List, get, and search contacts. |
| `apple-messages` | List chats/history, send messages, manage local chat aliases. |
| `apple-notes` | List, get, search, create, update, and append notes. |
| `apple-reminders` | List/read reminders and create, update, complete, or delete them. |

Some old compatibility calls are implemented but hidden from `tools/list`.
Use the advertised names for new work.

Mail, Contacts, Notes, and Reminders need macOS Automation permission.
Messages history also needs Full Disk Access. Read or write this data only
after the user gives clear permission.

There is no Apple Calendar connector. Use `caldav` or `google-calendar`.

Code: `rzn_tools_core/src/connectors/apple_*`

