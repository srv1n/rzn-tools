# RZN Integrations Authentication Architecture

This document describes the authentication architecture for **RZN Integrations** (`rzn-tools`),
particularly for desktop applications that manage credentials in their own secure storage.

## Overview

RZN Integrations connectors accept credentials via `AuthDetails`, a simple
`HashMap<String, String>`. This allows downstream apps to:
1. Store credentials in their own secure storage (encrypted database, keychain, etc.)
2. Pass credentials at runtime without `rzn-tools` persisting them
3. Share credentials across multiple connectors where applicable

## Core Types

```rust
// From rzn_tools_core/src/auth.rs
pub type AuthDetails = HashMap<String, String>;

// From rzn_tools_core/src/capabilities.rs
pub struct ConnectorConfigSchema {
    pub fields: Vec<Field>,
}

pub struct Field {
    pub name: String,           // Field key in AuthDetails
    pub label: String,          // Human-readable label
    pub field_type: FieldType,  // Text, Secret, Number, Boolean, Select
    pub required: bool,
    pub description: Option<String>,
    pub options: Option<Vec<String>>,
}

pub enum FieldType {
    Text,
    Secret,  // API keys, passwords, tokens
    Number,
    Boolean,
    Select { options: Vec<String> },
}
```

## Credential Lookup Priority

Connectors resolve credentials in this order:
1. **Runtime AuthDetails** - Passed to connector constructor or `set_auth_details()`
2. **Environment Variables** - Connector-specific (see table below)
3. **FileAuthStore** - `~/.config/rzn-tools/auth.json` (CLI default)

For desktop apps, you'll typically use option 1 to pass credentials from your encrypted database.

## Connector Credential Requirements

### API Key Connectors (Single Token)

These connectors need a single API key/token. The key name in `AuthDetails` varies by connector.

| Connector | AuthDetails Key | Environment Variable | Obtain URL |
|-----------|-----------------|---------------------|------------|
| Slack | `token` | `SLACK_BOT_TOKEN` | https://api.slack.com/apps |
| GitHub | `token` | `GITHUB_TOKEN` | https://github.com/settings/tokens |
| OpenAI | `api_key` | `OPENAI_API_KEY` | https://platform.openai.com/api-keys |
| Anthropic | `api_key` | `ANTHROPIC_API_KEY` | https://console.anthropic.com/settings/keys |
| Perplexity | `api_key` | `PPLX_API_KEY` | https://www.perplexity.ai/settings/api |
| Exa | `api_key` | `EXA_API_KEY` | https://dashboard.exa.ai/api-keys |
| Tavily | `api_key` | `TAVILY_API_KEY` | https://tavily.com/#api |
| Brave Search | `api_key` | `BRAVE_API_KEY` | https://brave.com/search/api/ |
| Firecrawl | `api_key` | `FIRECRAWL_API_KEY` | https://firecrawl.dev |
| SerpApi | `api_key` | `SERPAPI_API_KEY` | https://serpapi.com |
| xAI | `api_key` | `XAI_API_KEY` | https://x.ai |
| Gemini | `api_key` | `GEMINI_API_KEY` | https://aistudio.google.com/apikey |
| X (Twitter) API | see X section below | bearer + OAuth 2.0 + OAuth 1.0a env vars | https://developer.x.com |

**Example:**
```rust
let mut auth = AuthDetails::new();
auth.insert("api_key".to_string(), "sk-...".to_string());
connector.set_auth_details(auth).await?;
```

### Multi-Field Connectors

These require multiple credential fields.

#### Reddit
| Field | Required | Description |
|-------|----------|-------------|
| `client_id` | Yes | Reddit app client ID |
| `client_secret` | Yes | Reddit app secret |
| `username` | Optional | For authenticated actions |
| `password` | Optional | For authenticated actions |

Note: Reddit works without auth for public content. Auth enables posting and private subreddits.

#### Google Custom Search
| Field | Required | Description |
|-------|----------|-------------|
| `api_key` | Yes | Google Cloud API key |
| `cse_id` | Yes | Custom Search Engine ID |

#### IMAP Email
| Field | Required | Description |
|-------|----------|-------------|
| `host` | Yes | IMAP server hostname |
| `port` | No | Default: 993 |
| `username` | Yes | Email address |
| `password` | Yes | Password or app-specific password |
| `use_tls` | No | Default: true |

### OAuth Connectors

These use OAuth 2.0 and store multiple token-related fields.

#### Google Services (Drive, Gmail, Calendar, People)

| Field | Description |
|-------|-------------|
| `client_id` | OAuth client ID |
| `client_secret` | OAuth client secret (optional for public clients) |
| `access_token` | Short-lived access token |
| `refresh_token` | Long-lived refresh token |
| `expires_at` | Unix timestamp when access_token expires |

**Token Refresh:** Use `rzn_tools_core::oauth::ensure_google_access(&mut auth)` to automatically refresh expired tokens. This modifies the AuthDetails in-place with new tokens.

#### Microsoft Graph (OneDrive, Outlook, Calendar)

| Field | Description |
|-------|-------------|
| `client_id` | Azure AD app client ID |
| `client_secret` | Optional |
| `tenant_id` | Azure tenant ID (or "common") |
| `access_token` | Short-lived access token |
| `refresh_token` | Long-lived refresh token |
| `expires_at` | Unix timestamp when access_token expires |

**Token Refresh:** Use `rzn_tools_core::oauth::ensure_ms_access(&mut auth)`.

#### X (Twitter)

X is a multi-field connector, not a single bearer token. Keep the credential family aligned with
the use case:

| Mode | AuthDetails Keys | Environment Variables | Use for |
|------|------------------|----------------------|---------|
| Bearer | `bearer_token` | `X_BEARER_TOKEN` / `TWITTER_BEARER_TOKEN` | public read endpoints |
| OAuth 2.0 PKCE | `oauth2_access_token`, `oauth2_refresh_token`, `oauth2_expires_at`, `oauth2_scope`, `oauth2_token_type`, `client_id`, optional `client_secret`, `redirect_uri` | `X_OAUTH2_ACCESS_TOKEN`, `X_OAUTH2_REFRESH_TOKEN`, `X_OAUTH2_EXPIRES_AT`, `X_OAUTH2_SCOPE`, `X_OAUTH2_TOKEN_TYPE`, `X_CLIENT_ID`, `X_CLIENT_SECRET`, `X_REDIRECT_URI` | user-context reads and writes |
| OAuth 1.0a | `oauth1_consumer_key`, `oauth1_consumer_secret`, `oauth1_access_token`, `oauth1_access_token_secret` | `X_OAUTH_CONSUMER_KEY`, `X_OAUTH_CONSUMER_SECRET`, `X_OAUTH_ACCESS_TOKEN`, `X_OAUTH_ACCESS_TOKEN_SECRET` | legacy import or endpoint fallback |

Recommended order:

1. Use bearer for public reads.
2. Use OAuth 2.0 PKCE for anything that acts as a user.
3. Keep OAuth 1.0a only for fallback/import cases.

Validation flow:

```bash
rzn-tools setup x
rzn-tools x auth-status

# If you imported OAuth user tokens:
rzn-tools x whoami
rzn-tools x refresh-oauth2
```

MCP flow:

- Provide the same fields through environment variables or connector auth storage.
- Validate with `x/get_auth_status`.
- Validate user-context grants with `x/whoami`.

### Browser Cookie Connectors

These extract session cookies from browsers. Primarily for services without official APIs.

#### X (Browser Cookies) (`x-browser`)
| Field | Description |
|-------|-------------|
| `browser` | Browser to extract from: `chrome`, `firefox`, `safari`, `brave` |

Alternative (manual credentials):
| Field | Description |
|-------|-------------|
| `username` | X username |
| `password` | X password |
| `email` | Email for verification |
| `2fa_secret` | TOTP secret for 2FA |

## Credential Sharing Opportunities

### Same Provider, Multiple Connectors

These credentials can be shared across connectors from the same provider:

| Credential | Can Be Shared With |
|------------|-------------------|
| `OPENAI_API_KEY` | openai-search, (future: openai-chat, openai-embeddings) |
| `ANTHROPIC_API_KEY` | anthropic-search, (future: anthropic-chat) |
| Google OAuth tokens | google-drive, google-gmail, google-calendar, google-people |
| Microsoft OAuth tokens | microsoft-graph covers OneDrive, Outlook, Calendar |

**Example: Sharing OpenAI credentials**
```rust
// In your desktop app
let openai_key = encrypted_db.get("openai_api_key")?;

// Use for LLM calls
let llm_client = OpenAIClient::new(&openai_key);

// Use for rzn-tools search connector
let mut auth = AuthDetails::new();
auth.insert("api_key".to_string(), openai_key);
let openai_search = registry.get_provider("openai-search")?;
openai_search.lock().await.set_auth_details(auth).await?;
```

### Google Services Consolidation

A single OAuth consent can cover multiple Google APIs. Request all needed scopes upfront:

```rust
let scopes = vec![
    "https://www.googleapis.com/auth/drive.readonly",
    "https://www.googleapis.com/auth/gmail.readonly",
    "https://www.googleapis.com/auth/calendar.readonly",
].join(" ");

// One OAuth flow, use tokens for all Google connectors
let tokens = do_google_oauth(&client_id, &scopes).await?;

// Same tokens work for all
for connector_name in ["google-drive", "google-gmail", "google-calendar"] {
    let connector = registry.get_provider(connector_name)?;
    connector.lock().await.set_auth_details(tokens.clone()).await?;
}
```

## Integration Patterns for Desktop Apps

### Pattern 1: Pass Credentials at Runtime

```rust
use rzn_tools_core::{auth::AuthDetails, build_registry_enabled_only};

// Load from your encrypted database
let credentials = your_secure_storage.load_credentials("slack")?;

// Build registry (connectors start without auth)
let registry = build_registry_enabled_only().await;

// Set auth before use
let slack = registry.get_provider("slack")?;
{
    let mut connector = slack.lock().await;
    let mut auth = AuthDetails::new();
    auth.insert("token".to_string(), credentials.token);
    connector.set_auth_details(auth).await?;
}

// Now use the connector
let result = slack.lock().await.call_tool(request).await?;
```

### Pattern 2: Credential Verification UI

Query the connector's schema to build a dynamic credential form:

```rust
let connector = registry.get_provider("slack")?.lock().await;
let schema = connector.config_schema();

for field in schema.fields {
    match field.field_type {
        FieldType::Secret => {
            // Render password input
            ui.password_field(&field.label, &field.description);
        }
        FieldType::Text => {
            // Render text input
            ui.text_field(&field.label, &field.description);
        }
        FieldType::Select { options } => {
            // Render dropdown
            ui.dropdown(&field.label, &options);
        }
        // ...
    }
}
```

### Pattern 3: Test Before Save

Always test credentials before storing:

```rust
let mut auth = AuthDetails::new();
auth.insert("token".to_string(), user_input);

connector.set_auth_details(auth.clone()).await?;

match connector.test_auth().await {
    Ok(_) => {
        // Credentials valid, save to encrypted storage
        your_secure_storage.save("slack", &auth)?;
        show_success("Connected to Slack!");
    }
    Err(e) => {
        show_error(&format!("Authentication failed: {}", e));
    }
}
```

### Pattern 4: OAuth in Desktop App

For OAuth connectors, you can either:

**A) Use Device Authorization Flow (recommended for desktop)**
```rust
use rzn_tools_core::oauth::{google_device_authorize, google_device_poll};

// Start device auth
let device = google_device_authorize(&client_id, &scopes).await?;

// Show UI: "Go to {device.verification_uri} and enter code {device.user_code}"
show_auth_dialog(&device.verification_uri, &device.user_code);

// Poll for completion
loop {
    tokio::time::sleep(Duration::from_secs(device.interval.unwrap_or(5))).await;

    match google_device_poll(&client_id, client_secret.as_deref(), &device.device_code).await {
        Ok(tokens) => {
            // Save tokens to encrypted storage
            save_oauth_tokens("google", &tokens)?;
            break;
        }
        Err(e) if e.to_string().contains("authorization_pending") => continue,
        Err(e) => return Err(e),
    }
}
```

**B) Use System Browser with Localhost Callback**
```rust
// Start local server on random port
let listener = TcpListener::bind("127.0.0.1:0")?;
let port = listener.local_addr()?.port();
let redirect_uri = format!("http://localhost:{}/callback", port);

// Open browser to auth URL
let auth_url = format!(
    "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&scope={}&response_type=code",
    client_id, redirect_uri, scopes
);
open::that(&auth_url)?;

// Wait for callback with authorization code
let code = wait_for_callback(listener).await?;

// Exchange code for tokens
let tokens = exchange_code_for_tokens(&client_id, &client_secret, &code, &redirect_uri).await?;
```

## Token Storage Recommendations

### For Desktop Apps

1. **Use OS Keychain/Credential Manager**
   - macOS: Keychain Services
   - Windows: Credential Manager / DPAPI
   - Linux: libsecret / GNOME Keyring

2. **Or Encrypted Database**
   - SQLCipher for SQLite encryption
   - Encrypt at rest with user-derived key

3. **Token Structure**
   ```rust
   struct StoredCredential {
       connector: String,
       auth_type: AuthType,  // ApiKey, OAuth, Cookies
       fields: HashMap<String, String>,  // The AuthDetails
       created_at: DateTime<Utc>,
       updated_at: DateTime<Utc>,
       // For OAuth
       expires_at: Option<DateTime<Utc>>,
   }
   ```

4. **Refresh Token Handling**
   - Store refresh tokens securely
   - Refresh access tokens proactively (before expiry)
   - Handle refresh failures gracefully (re-auth flow)

## Error Handling

```rust
pub enum ConnectorError {
    Authentication(String),  // Invalid/expired credentials
    // ... other variants
}
```

Common auth errors:
- `Authentication("Missing token")` - Required credential not provided
- `Authentication("Invalid token")` - Credential rejected by service
- `Authentication("Token expired")` - OAuth access token expired (needs refresh)
- `Authentication("Refresh failed")` - Refresh token invalid (needs re-auth)

## Security Considerations

1. **Never log credentials** - AuthDetails may contain secrets
2. **Clear memory after use** - Consider `secrecy` crate for sensitive strings
3. **Validate before use** - Always call `test_auth()` after setting credentials
4. **Scope minimally** - Request only needed OAuth scopes
5. **Rotate regularly** - Implement credential rotation for long-running apps
6. **Handle revocation** - Users may revoke access; handle gracefully

## Quick Reference: Connector → AuthDetails Keys

```
slack:            { token }
github:           { token }
openai-search:    { api_key }
anthropic-search: { api_key }
perplexity:       { api_key }
reddit:           { client_id, client_secret, username?, password? }
google-*:         { client_id, client_secret?, access_token, refresh_token, expires_at }
microsoft-graph:  { client_id, client_secret?, tenant_id?, access_token, refresh_token, expires_at }
x:                { browser } OR { username, password, email?, 2fa_secret? }
imap:             { host, port?, username, password, use_tls? }
```

## Questions for Downstream Team

1. **Credential UI**: Will you build a generic credential form from `config_schema()` or hardcode per-connector UIs?

2. **OAuth Flow**: Do you prefer device authorization (simpler) or browser callback (smoother UX)?

3. **Token Refresh**: Should rzn-tools auto-refresh, or will you handle refresh in your app layer?

4. **Credential Sharing**: Which providers do your users commonly have? (Helps prioritize shared credential support)

5. **Browser Cookies**: Do you want to support browser cookie extraction, or skip services that require it?
