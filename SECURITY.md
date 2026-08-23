# Security

Do not open a public issue for a vulnerability. Use GitHub private vulnerability reporting for this repository.

Include:

- the affected code or tool
- steps to reproduce
- the expected and actual result
- the possible impact
- a proposed fix, when known

## Current boundaries

- `auth.json` contains plain JSON credentials.
- Unix systems set this file to mode `0600`.
- Some connectors read environment variables. The names are connector-specific.
- The MCP HTTP server has no built-in user authentication, TLS, or rate limit.
- `localfs` has no root allowlist or path sandbox.
- Several connectors can write, send, upload, submit, cancel, or delete data.

Use least-privilege tokens. Keep the MCP server local unless an authenticated TLS proxy protects it.

See the [Security guide](docs/system/security.md) for operational rules.
