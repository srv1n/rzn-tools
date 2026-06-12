# RZNApp Connectors Contract: Auth Slots + Icons + Config Credentials

Date: 2026-02-13

This doc defines the contract rzn-tools should support (directly or via `rzn-tools-mcp`) so that:

- `rznapp` can render a **non-technical Connectors UI** without bespoke per-connector code.
- Protected actions (write/post/delete) are clearly gated behind “Sign in” flows.
- Multi-field “credentials” like IMAP work cleanly without special casing.
- Icons flow end-to-end into the Connectors and Tool Catalog UIs.

This is a contract + decision record; it also lists implementation tasks for rzn-tools.

Related global memo (desktop-side):
In the `rznapp` repo: `docs/design/EXTENSIONS_BUILD_SIGN_PUBLISH_INSTALL_STRATEGY_MEMO_2026-02-13.md`

---

## 0) Context: What Problem We’re Solving

Users don’t want to think about:

- MCP servers
- auth protocols
- per-tool nuances

They want:

- “Connect Reddit”
- “Test”
- “Enable only the tools I want”
- “Post actions require sign-in; read actions don’t”

rzn-tools already has the real knowledge (auth requirements, tool groupings). The host should not re-invent it.

---

## 1) Stable Vocabulary

- **Connector**: user-facing “integration” grouping (Reddit, Hacker News, IMAP, PubMed).
- **Tool/Action**: a callable operation inside a connector.
- **Auth slot**: a named “account/config requirement” a connector needs.
  - Example: `reddit_account` for write actions.
- **Credential**: reusable secret/config object stored in the host (encrypted).
  - Examples: API key, OAuth sign-in, IMAP mailbox config.

---

## 2) Core Auth Modeling (Decision)

We hard-separate:

1) `provider_id` (stable service identity)
2) `auth_method_id` (provider-scoped “productized method”)
3) `auth_kind` (protocol / secret-shape)

Why:

- Avoids “Claude Max” vs “Anthropic” becoming different providers.
- Avoids overloading auth differences into provider names or connector ids.
- Keeps credential pickers stable and reusable across LLMs + Connectors + Agents.

### 2.1 Examples

OpenAI:
- `provider_id=openai`
- `auth_method_id=api_key` (`auth_kind=api_key`)
- (later) `auth_method_id=chatgpt_sign_in` (`auth_kind=oauth2`)

Anthropic:
- `provider_id=anthropic`
- `auth_method_id=api_key` (`auth_kind=api_key`)
- `auth_method_id=claude_sign_in` (`auth_kind=oauth2`)

Reddit:
- `provider_id=reddit`
- `auth_method_id=sign_in` (`auth_kind=oauth2`)

IMAP:
- `provider_id=imap`
- `auth_method_id=mailbox` (`auth_kind=config`)

---

## 3) UI-Driven Metadata: What rzn-tools Must Provide

The host needs a single “connector catalog” view that includes:

### 3.1 Connector-level metadata

Required:

- `connector_id` (stable)
- `display_name`
- `icon_url` (optional, but strongly recommended)
- `supports_anonymous_read` (boolean)
- `requires_auth` (boolean; true if any action requires auth)

Optional:

- `description` (short)
- `category` (search, social, email, docs, developer)

### 3.2 Auth slots (required when any protected action exists)

A connector defines one or more auth slots:

- `slot_id` (stable string)
- `provider_id`
- `auth_method_id`
- `auth_kind` (`api_key|oauth2|config`)
- `scopes` (optional; oauth2 only)
- `config_schema` (required when `auth_kind=config`)

The host renders:

- “Sign in” / “Reconnect” / “Disconnect”
- “Test” buttons (invokes host test command)
- a credential picker (filtered to `(provider_id, auth_method_id, auth_kind)`)

### 3.3 Actions/tools metadata

Each action needs:

- `tool_id` (stable)
- `name`
- `description`
- `operation` (read/write/admin; for copy/UX only)
- `enabled` (host-managed)
- `available` (host-computed: true if prerequisites met)
- `requires_slot_id` (optional; if present, tool is blocked until slot is connected)
- `icons` (optional; used in Tool Catalog UI)

Key rule:

- Tools must reference **slots**, not “oauth”.
  - Example: “Reply to comment” -> `requires_slot_id=reddit_account`

---

## 4) Config Credentials (IMAP and other multi-field auth)

For IMAP-like connectors, a single string API key does not work. We need:

- `auth_kind=config`
- `config_schema` describing fields and which are secrets

### 4.1 Schema format

Use JSON Schema-like shape sufficient for rendering a form:

- field `type` (string/number/boolean/enum)
- `title`/`description`
- `secret: true` for sensitive fields (passwords)
- validation (min/max/required)

Example (illustrative):

```json
{
  "type": "object",
  "required": ["host","port","username","password"],
  "properties": {
    "host": { "type": "string", "title": "Host" },
    "port": { "type": "number", "title": "Port" },
    "username": { "type": "string", "title": "Username" },
    "password": { "type": "string", "title": "Password", "secret": true }
  }
}
```

Host behavior:

- store the config as an encrypted JSON object in Credentials Directory
- bind connector slot to a credential id (alias-aware)
- pass credential handle to rzn-tools at runtime

This pattern generalizes to SMTP, databases, internal services, etc.

---

## 5) Icon Contract (Connectors + Tools)

Users scan by icons. We want icons to flow without special casing in host.

Requirements:

- connector `icon_url` should be either:
  - `https://...` (preferred), or
  - `data:image/...` (acceptable for bundled icons)
- tool `icons` should include at least one icon (optional but recommended)

Fallback rules (host-side):

- if missing, host generates a letter badge.
- if invalid URL, host logs once and falls back.

rzn-tools action item:

- ensure connector metadata includes a branded icon where available.
- ensure tool metadata includes `icons` for Tool Catalog display (if present in rzn-tools already, confirm it’s wired end-to-end).

---

## 6) Responsibilities Split (Host vs rzn-tools)

### Host owns

- OAuth browser flow (PKCE, redirect handling)
- token storage + refresh + reauth UX
- credentials directory (encrypt at rest)
- tool enable/disable switches (context bloat control)

### rzn-tools owns

- connector tool implementations
- declaring auth slots + which tools require which slots
- connector icons + tool icons

---

## 7) Multi-account / Alias Strategy (Launch)

We support multi-account via **alias**:

- default alias: `default`
- user can create a second credential with alias `work` or `gmail2`, etc

Launch constraint (recommended):

- one active credential per `(connector_id, slot_id, alias)` binding
- UI supports selecting which alias you’re editing via a query param (deep links from Credentials “Manage”)

---

## 8) Implementation Tasks (rzn-tools)

### P1: Auth slots metadata

- For each connector, define:
  - `supports_anonymous_read`
  - `auth_slots[]` with `(slot_id, provider_id, auth_method_id, auth_kind, config_schema?)`
  - map protected tools to `requires_slot_id`

Acceptance:
- Host can show “read works, write needs sign-in” without guessing.

### P1: Icon propagation

- Ensure connector `icon_url` is present (where possible)
- Ensure tool `icons` propagate to the MCP tool list / metadata surface

Acceptance:
- `rznapp` Connectors list shows icons (not initials) for rzn-tools-provided connectors.

### P2: Config credentials schema (IMAP)

- Provide IMAP connector slot:
  - `provider_id=imap`
  - `auth_method_id=mailbox`
  - `auth_kind=config`
  - `config_schema` fields: host/port/username/password/tls

Acceptance:
- Host wizard renders an IMAP form without bespoke UI.

### P2: rzn-tools-mcp packaging plan alignment (if rzn-tools becomes a plugin)

If we move rzn-tools to installable extensions later:

- `rzn-tools-mcp` must still expose the same metadata contract
- plugin packaging should bundle rzn-tools + connector definitions

Acceptance:
- `rznapp` treats rzn-tools connectors the same whether built-in or via MCP worker.
