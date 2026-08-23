---
subject: security
keywords: [secrets, privacy, credentials, http]
part_of: overview
describes: [.env.example, rzn_tools_core/src/auth_store.rs, rzn_tools_core/src/oauth_client.rs, rzn_tools_mcp/src/http.rs, SECURITY.md]
status: canonical
created: 2026-08-23
last_verified: 2026-08-23 @ working-tree
read_when: "You handle credentials, personal data, or a network server."
skip_when: "You need normal command examples only. Open cli.md."
---

# Security

Protect credentials, private data, and network access. Give each process and
connector only the access that it needs.

## Credentials

Use `rzn-tools setup` or environment values that the selected connector reads.
The auth store is `rzn-tools/auth.json` below the operating system config
directory. On Unix the file mode is set to `0600`. The JSON values are not
encrypted.

Never put a real secret in source, `.env.example`, tests, fixtures, logs,
documentation, or Tusker tasks. Do not copy an auth file between users.

Use one profile for each account. Select a CLI profile with
`--auth-profile <name>`. MCP has no equivalent profile flag. Some OAuth device
flows save refreshed tokens only when `RZN_PERSIST_TOKENS=1`.

## Personal data

Mail, messages, contacts, reminders, calendars, cloud files, local files, and
social accounts can contain private data. Use these connectors only with clear
user permission. Do not call them in automated tests.

`localfs` reads paths given to it. It has no built-in root allowlist or path
sandbox. Restrict the process account and the paths that callers can provide.

## HTTP server

The MCP HTTP server has no built-in user authentication, TLS, or rate limit.
Its host allowlist is opt-in and is not an access-control system.

For network use:

1. Put the server behind an authenticated TLS proxy.
2. Set an exact host allowlist.
3. Expose only the required connectors.
4. Bind to a private interface.
5. Review logs before you share them.

Do not publish a raw Cloudflare tunnel to an untrusted audience. A public URL
does not add authentication by itself.

## Provider and write risks

Some tools create, update, delete, send, or cancel remote data. Read the tool
schema and confirm the target before a write. Provider terms, rate limits, and
access controls still apply.

For App Store Connect analytics, use only Apple-provided segment URLs. The
current download path sends the Apple bearer token to the given segment host.

Return provider errors instead of hiding them. Use small request limits and
backoff for new provider calls.
