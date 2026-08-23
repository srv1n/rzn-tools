---
title: "Security"
subject: security
keywords: [credentials, privacy, http, writes]
part_of: overview
describes: [.env.example, rzn_tools_core/src/auth_store.rs, rzn_tools_mcp/src/http.rs, SECURITY.md]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You handle credentials, private data, or a network server."
skip_when: "You only need public read examples."
---

# Security

Treat each connector as a direct path to its source. A tool can read private data or change remote state.

## Credentials

`rzn-tools setup` stores connector fields in `auth.json` under the operating system config directory. Unix systems set file mode `0600`.

The file is plain JSON. It is not encrypted.

Some connectors also read environment variables. The accepted names are connector-specific. Check the connector source or config schema.

Do not place real secrets in source, examples, tests, logs, documents, or Tusker records.

Named CLI profiles are not uniform across all connectors. Some Google and Microsoft paths read their default store keys directly.

## Private data

Mail, messages, contacts, reminders, calendars, cloud files, local files, and social accounts can contain private data. Use these connectors only with clear user permission.

Do not call personal-data connectors in automated tests.

`localfs` reads caller-provided paths. It has no root allowlist and no path sandbox. Restrict the process account and caller input.

## HTTP server

The MCP HTTP server has no built-in user authentication, TLS, or rate limit. Its host allowlist is not user authentication.

For network use:

1. Put the server behind an authenticated TLS proxy.
2. Set an exact host allowlist.
3. Expose only required connectors.
4. Bind to a private interface.
5. Review logs before you share them.

A public tunnel does not add authentication.

## Writes

Several tools create, update, delete, send, upload, submit, or cancel data. Read the live tool schema before a call.

Confirm these values before a write:

- connector
- account
- target ID
- request body
- dry-run setting, when present

A missing dry-run option means the operation is live.

Provider access rules, rate limits, and terms still apply.
