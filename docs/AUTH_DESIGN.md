# Authentication design

The current security and auth guide is [system/security.md](system/security.md).

The implementation is in:

- `rzn_tools_core/src/auth.rs`
- `rzn_tools_core/src/auth_store.rs`
- `rzn_tools_core/src/oauth.rs`
- `rzn_tools_core/src/oauth_client.rs`
- `rzn_tools_cli/src/commands/setup.rs`
- `rzn_tools_cli/src/commands/config.rs`

Auth values are stored as plain JSON below the platform config directory.
Profiles use `provider::profile` keys. Environment support is connector
specific; it is not a universal fallback rule. Use `rzn-tools setup
<connector>` and the live config schema.
