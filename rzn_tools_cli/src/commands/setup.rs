use crate::cli::Cli;
use crate::commands::{CommandError, Result};
use owo_colors::OwoColorize;
use rzn_tools_core::{
    auth::AuthDetails,
    auth_store::FileAuthStore,
    oauth::{google_device_authorize, google_device_poll, ms_device_authorize, ms_device_poll},
    PaginatedRequestParam,
};
use std::io::{self, Write};

/// Connector configuration metadata
struct ConnectorSetupInfo {
    name: &'static str,
    display_name: &'static str,
    description: &'static str,
    auth_type: AuthType,
    env_vars: &'static [(&'static str, &'static str)], // (env_var, description)
    required_fields: &'static [FieldInfo],
    instructions: Option<SetupInstructions>,
    aliases: &'static [&'static str], // Alternative names for this connector
}

struct FieldInfo {
    name: &'static str,
    label: &'static str,
    is_secret: bool,
    hint: Option<&'static str>, // e.g., "starts with xoxb-"
}

struct SetupInstructions {
    obtain_url: &'static str,
    steps: &'static [&'static str],
}

#[derive(Clone, Copy)]
enum AuthType {
    None,
    ApiKey,
    OAuth { provider: OAuthProvider },
    BrowserCookies,
    MultipleFields,
}

#[derive(Clone, Copy)]
enum OAuthProvider {
    Google { scopes: &'static str },
    Microsoft { scopes: &'static str },
}

const CONNECTORS: &[ConnectorSetupInfo] = &[
    // === No Auth Required ===
    ConnectorSetupInfo {
        name: "youtube",
        display_name: "YouTube",
        description: "Video details, transcripts, and search",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "hackernews",
        display_name: "Hacker News",
        description: "Tech news and discussions",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "arxiv",
        display_name: "ArXiv",
        description: "Academic preprints and papers",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "wikipedia",
        display_name: "Wikipedia",
        description: "Encyclopedia articles",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "pubmed",
        display_name: "PubMed",
        description: "Medical and life science literature",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "biorxiv",
        display_name: "bioRxiv/medRxiv",
        description: "Biology and Health Sciences Preprints",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "rss",
        display_name: "RSS",
        description: "RSS/Atom feed reader",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "google-scholar",
        display_name: "Google Scholar",
        description: "Academic papers via Google Scholar (scraping)",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "app-store",
        display_name: "App Store",
        description: "Public App Store app metadata (iTunes Search API) + reviews (RSS feed)",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &["appstore", "itunes"],
    },
    ConnectorSetupInfo {
        name: "polymarket",
        display_name: "Polymarket",
        description: "Public prediction-market events and markets via the Gamma API",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "kalshi",
        display_name: "Kalshi",
        description: "Public Kalshi series, events, markets, order books, and price history",
        auth_type: AuthType::None,
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    // === API Key Auth ===
    ConnectorSetupInfo {
        name: "discord",
        display_name: "Discord",
        description: "Discord server messages and channels",
        auth_type: AuthType::ApiKey,
        env_vars: &[("DISCORD_TOKEN", "Bot Token")],
        required_fields: &[FieldInfo {
            name: "token",
            label: "Bot Token",
            is_secret: true,
            hint: None,
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://discord.com/developers/applications",
            steps: &[
                "Create a New Application",
                "Go to the 'Bot' tab",
                "Click 'Reset Token' to get your token",
                "Ensure 'Message Content Intent' is enabled",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "slack",
        display_name: "Slack",
        description: "Workspace messages and channels",
        auth_type: AuthType::ApiKey,
        env_vars: &[("SLACK_BOT_TOKEN", "Bot Token")],
        required_fields: &[FieldInfo {
            name: "token",
            label: "Bot Token",
            is_secret: true,
            hint: Some("starts with xoxb-"),
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://api.slack.com/apps",
            steps: &[
                "Create a new app or select an existing one",
                "Go to 'OAuth & Permissions' in the sidebar",
                "Add required scopes: channels:read, channels:history, users:read",
                "Install the app to your workspace",
                "Copy the 'Bot User OAuth Token' (starts with xoxb-)",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "github",
        display_name: "GitHub",
        description: "Repositories, issues, and PRs",
        auth_type: AuthType::ApiKey,
        env_vars: &[("GITHUB_TOKEN", "Personal Access Token")],
        required_fields: &[FieldInfo {
            name: "token",
            label: "Personal Access Token",
            is_secret: true,
            hint: Some("starts with ghp_ or github_pat_"),
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://github.com/settings/tokens",
            steps: &[
                "Click 'Generate new token' (classic or fine-grained)",
                "Select scopes: repo, read:org (for private repos)",
                "Generate and copy the token",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "brave_search",
        display_name: "Brave Search",
        description: "Privacy-focused web search",
        auth_type: AuthType::ApiKey,
        env_vars: &[("BRAVE_API_KEY", "API Key")],
        required_fields: &[FieldInfo {
            name: "api_key",
            label: "API Key",
            is_secret: true,
            hint: None,
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://brave.com/search/api/",
            steps: &[
                "Sign up for a Brave Search API account",
                "Navigate to the API dashboard",
                "Create a new API key",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "openai-search",
        display_name: "OpenAI Web Search",
        description: "Web search via OpenAI",
        auth_type: AuthType::ApiKey,
        env_vars: &[("OPENAI_API_KEY", "API Key")],
        required_fields: &[FieldInfo {
            name: "api_key",
            label: "API Key",
            is_secret: true,
            hint: Some("starts with sk-"),
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://platform.openai.com/api-keys",
            steps: &[
                "Log in to your OpenAI account",
                "Navigate to API Keys section",
                "Create a new secret key",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "anthropic-search",
        display_name: "Claude Web Search",
        description: "Web search via Claude",
        auth_type: AuthType::ApiKey,
        env_vars: &[("ANTHROPIC_API_KEY", "API Key")],
        required_fields: &[FieldInfo {
            name: "api_key",
            label: "API Key",
            is_secret: true,
            hint: Some("starts with sk-ant-"),
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://console.anthropic.com/settings/keys",
            steps: &[
                "Log in to your Anthropic Console",
                "Navigate to API Keys",
                "Create a new key",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "bing-webmaster-tools",
        display_name: "Bing Webmaster Tools",
        description: "SEO performance + URL submission",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("BING_WEBMASTER_API_KEY", "API Key"),
            ("INDEXNOW_KEY", "IndexNow key (optional)"),
            ("INDEXNOW_KEY_LOCATION", "IndexNow key file URL (optional)"),
        ],
        required_fields: &[
            FieldInfo {
                name: "api_key",
                label: "API Key",
                is_secret: true,
                hint: None,
            },
            FieldInfo {
                name: "indexnow_key",
                label: "IndexNow Key (optional)",
                is_secret: true,
                hint: Some("host at https://<host>/<key>.txt"),
            },
            FieldInfo {
                name: "indexnow_key_location",
                label: "IndexNow Key Location (optional)",
                is_secret: false,
                hint: Some("defaults to https://<host>/<key>.txt"),
            },
        ],
        instructions: Some(SetupInstructions {
            obtain_url:
                "https://github.com/srv1n/rzn-tools/blob/main/docs/connectors/bing_webmaster_tools.md",
            steps: &[
                "Verify ownership of your site in Bing Webmaster Tools",
                "Go to Settings → API Access",
                "Generate an API key",
                "Optional: set up IndexNow by hosting the key file at https://<host>/<key>.txt",
            ],
        }),
        aliases: &["bing-webmaster", "bing-search-console", "bing-webmasters"],
    },
    ConnectorSetupInfo {
        name: "perplexity-search",
        display_name: "Perplexity Search",
        description: "AI-powered web search",
        auth_type: AuthType::ApiKey,
        env_vars: &[("PPLX_API_KEY", "API Key")],
        required_fields: &[FieldInfo {
            name: "api_key",
            label: "API Key",
            is_secret: true,
            hint: Some("starts with pplx-"),
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://www.perplexity.ai/settings/api",
            steps: &[
                "Log in to Perplexity",
                "Go to Settings > API",
                "Generate a new API key",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "xai-search",
        display_name: "xAI Search",
        description: "Grok live web+X search via xAI (grounded, with citations)",
        auth_type: AuthType::ApiKey,
        env_vars: &[("XAI_API_KEY", "API Key")],
        required_fields: &[FieldInfo {
            name: "api_key",
            label: "API Key",
            is_secret: true,
            hint: Some("starts with xai-"),
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://docs.x.ai/developers/quickstart",
            steps: &[
                "Create an xAI account and add credits in the xAI console",
                "Open the API Keys page and generate an API key",
                "Paste the key when prompted (or set XAI_API_KEY)",
                "Use 'rzn-tools xai-search search --query \"...\"' for web_search or x_search",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "exa",
        display_name: "Exa",
        description: "Advanced AI search with neural search, similarity finding, content extraction, and answer generation",
        auth_type: AuthType::ApiKey,
        env_vars: &[("EXA_API_KEY", "API Key")],
        required_fields: &[FieldInfo {
            name: "api_key",
            label: "API Key",
            is_secret: true,
            hint: None,
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://dashboard.exa.ai/api-keys",
            steps: &[
                "Sign up at exa.ai",
                "Navigate to the API Keys dashboard",
                "Create a new key",
            ],
        }),
        aliases: &["exa-search"],
    },
    ConnectorSetupInfo {
        name: "tavily-search",
        display_name: "Tavily Search",
        description: "AI search optimized for LLMs",
        auth_type: AuthType::ApiKey,
        env_vars: &[("TAVILY_API_KEY", "API Key")],
        required_fields: &[FieldInfo {
            name: "api_key",
            label: "API Key",
            is_secret: true,
            hint: Some("starts with tvly-"),
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://tavily.com/#api",
            steps: &[
                "Sign up at tavily.com",
                "Go to your dashboard",
                "Copy your API key",
            ],
        }),
        aliases: &["tavily"],
    },
    ConnectorSetupInfo {
        name: "parallel-search",
        display_name: "Parallel Search",
        description: "Advanced parallel web search",
        auth_type: AuthType::ApiKey,
        env_vars: &[("PARALLEL_API_KEY", "API Key")],
        required_fields: &[FieldInfo {
            name: "api_key",
            label: "API Key",
            is_secret: true,
            hint: None,
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://docs.parallel.ai",
            steps: &[
                "Sign up for a Parallel AI account",
                "Navigate to the dashboard",
                "Create a new API key",
            ],
        }),
        aliases: &["parallel"],
    },
    // === Multiple Fields ===
    ConnectorSetupInfo {
        name: "reddit",
        display_name: "Reddit",
        description: "Posts, comments, and subreddits (works without auth for public content)",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("REDDIT_CLIENT_ID", "Client ID"),
            ("REDDIT_CLIENT_SECRET", "Client Secret"),
        ],
        required_fields: &[
            FieldInfo {
                name: "client_id",
                label: "Client ID",
                is_secret: false,
                hint: Some("found under your app name"),
            },
            FieldInfo {
                name: "client_secret",
                label: "Client Secret",
                is_secret: true,
                hint: None,
            },
        ],
        instructions: Some(SetupInstructions {
            obtain_url: "https://www.reddit.com/prefs/apps",
            steps: &[
                "Scroll to 'Developed Applications' and click 'create app'",
                "Select 'script' as the app type",
                "Set redirect URI to http://localhost:8080",
                "Note the client ID (under app name) and secret",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "app-store-connect",
        display_name: "App Store Connect",
        description: "Developer-side API: apps, App Analytics reports, Sales & Finance reports",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("APP_STORE_CONNECT_KEY_ID", "Key ID"),
            ("APP_STORE_CONNECT_ISSUER_ID", "Issuer ID"),
            ("APP_STORE_CONNECT_P8_PATH", "Path to AuthKey_XXXXXX.p8"),
            ("APP_STORE_CONNECT_VENDOR_NUMBER", "Vendor Number (optional)"),
        ],
        required_fields: &[
            FieldInfo {
                name: "key_id",
                label: "Key ID",
                is_secret: false,
                hint: Some("looks like: ABC123DEFG"),
            },
            FieldInfo {
                name: "issuer_id",
                label: "Issuer ID",
                is_secret: false,
                hint: Some("UUID, e.g. 00000000-0000-0000-0000-000000000000"),
            },
            FieldInfo {
                name: "private_key_path",
                label: "Private Key Path (.p8)",
                is_secret: false,
                hint: Some("/absolute/path/to/AuthKey_ABC123DEFG.p8"),
            },
            FieldInfo {
                name: "vendor_number",
                label: "Vendor Number (optional)",
                is_secret: false,
                hint: None,
            },
        ],
        instructions: Some(SetupInstructions {
            obtain_url: "https://appstoreconnect.apple.com/access/api",
            steps: &[
                "App Store Connect → Users and Access → Keys",
                "Create an API key and download the .p8 private key",
                "Copy the Key ID and Issuer ID",
                "For Sales/Finance reports, find your Vendor Number in Agreements/Payments/Tax",
            ],
        }),
        aliases: &["asc", "appstoreconnect", "app_store_connect"],
    },
    ConnectorSetupInfo {
        name: "apple-search-ads",
        display_name: "Apple Search Ads",
        description: "Keyword recommendations and reporting via Search Ads API v5",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("ASA_ORG_ID", "Organization ID"),
            ("ASA_OAUTH_CLIENT_ID", "OAuth Client ID"),
            ("ASA_TEAM_ID", "Team ID"),
            ("ASA_KEY_ID", "Key ID"),
            ("ASA_P8_PATH", "Path to SearchAds_XXXXXX.p8"),
        ],
        required_fields: &[
            FieldInfo {
                name: "org_id",
                label: "Organization ID",
                is_secret: false,
                hint: None,
            },
            FieldInfo {
                name: "oauth_client_id",
                label: "OAuth Client ID",
                is_secret: false,
                hint: None,
            },
            FieldInfo {
                name: "team_id",
                label: "Team ID",
                is_secret: false,
                hint: Some("10-character Apple Developer Team ID"),
            },
            FieldInfo {
                name: "key_id",
                label: "Key ID",
                is_secret: false,
                hint: Some("looks like: ABC123DEFG"),
            },
            FieldInfo {
                name: "private_key_path",
                label: "Private Key Path (.p8)",
                is_secret: false,
                hint: Some("/absolute/path/to/SearchAds_ABC123DEFG.p8"),
            },
        ],
        instructions: Some(SetupInstructions {
            obtain_url: "https://searchads.apple.com/",
            steps: &[
                "Apple Search Ads → Account Settings → API",
                "Create an API client and download the .p8 private key",
                "Copy the OAuth Client ID, Team ID, and Key ID",
                "Copy your Organization ID (needed for X-AP-Context headers)",
            ],
        }),
        aliases: &["asa", "apple-searchads", "search-ads"],
    },
    ConnectorSetupInfo {
        name: "google_search",
        display_name: "Google Custom Search",
        description: "Web search via Google CSE",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("GOOGLE_API_KEY", "API Key"),
            ("GOOGLE_CSE_ID", "Custom Search Engine ID"),
        ],
        required_fields: &[
            FieldInfo {
                name: "api_key",
                label: "API Key",
                is_secret: true,
                hint: None,
            },
            FieldInfo {
                name: "cse_id",
                label: "Search Engine ID",
                is_secret: false,
                hint: Some("looks like: 017576662512468239146:omuauf_lfve"),
            },
        ],
        instructions: Some(SetupInstructions {
            obtain_url: "https://programmablesearchengine.google.com/",
            steps: &[
                "Create a Custom Search Engine at the URL above",
                "Get your Search Engine ID from the control panel",
                "Enable the Custom Search API in Google Cloud Console",
                "Create an API key in Google Cloud Console > Credentials",
            ],
        }),
        aliases: &[],
    },
    // === API Key ===
    ConnectorSetupInfo {
        name: "x",
        display_name: "X (Twitter) API",
        description: "Official X API v2 for reads, timelines, posting, likes, bookmarks, lists, DMs, and media (NOT xAI)",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("X_BEARER_TOKEN", "X API v2 bearer token"),
            ("TWITTER_BEARER_TOKEN", "X API v2 bearer token (legacy name)"),
            ("X_OAUTH2_ACCESS_TOKEN", "OAuth 2.0 access token"),
            ("X_OAUTH2_REFRESH_TOKEN", "OAuth 2.0 refresh token"),
            ("X_OAUTH2_EXPIRES_AT", "OAuth 2.0 expiry (epoch seconds or RFC3339)"),
            ("X_OAUTH2_SCOPE", "OAuth 2.0 scopes"),
            ("X_OAUTH2_TOKEN_TYPE", "OAuth 2.0 token type"),
            ("X_CLIENT_ID", "OAuth 2.0 client ID"),
            ("X_CLIENT_SECRET", "OAuth 2.0 client secret"),
            ("X_REDIRECT_URI", "OAuth 2.0 redirect URI"),
            ("X_OAUTH_CONSUMER_KEY", "OAuth 1.0a consumer key"),
            ("X_OAUTH_CONSUMER_SECRET", "OAuth 1.0a consumer secret"),
            ("X_OAUTH_ACCESS_TOKEN", "OAuth 1.0a access token"),
            (
                "X_OAUTH_ACCESS_TOKEN_SECRET",
                "OAuth 1.0a access token secret",
            ),
        ],
        required_fields: &[
            FieldInfo {
                name: "bearer_token",
                label: "Bearer Token",
                is_secret: true,
                hint: Some("public reads only; leave blank if you only have OAuth"),
            },
            FieldInfo {
                name: "oauth2_access_token",
                label: "OAuth2 Access Token",
                is_secret: true,
                hint: Some("preferred for user-context reads/writes"),
            },
            FieldInfo {
                name: "oauth2_refresh_token",
                label: "OAuth2 Refresh Token",
                is_secret: true,
                hint: Some("optional but strongly recommended"),
            },
            FieldInfo {
                name: "oauth2_expires_at",
                label: "OAuth2 Expires At",
                is_secret: false,
                hint: Some("epoch seconds or RFC3339"),
            },
            FieldInfo {
                name: "oauth2_scope",
                label: "OAuth2 Scope",
                is_secret: false,
                hint: Some("optional; e.g. tweet.read users.read tweet.write offline.access"),
            },
            FieldInfo {
                name: "oauth2_token_type",
                label: "OAuth2 Token Type",
                is_secret: false,
                hint: Some("optional; usually bearer"),
            },
            FieldInfo {
                name: "client_id",
                label: "Client ID",
                is_secret: false,
                hint: Some("needed for OAuth2 refresh"),
            },
            FieldInfo {
                name: "client_secret",
                label: "Client Secret",
                is_secret: true,
                hint: Some("optional; only if your OAuth client requires it"),
            },
            FieldInfo {
                name: "redirect_uri",
                label: "Redirect URI",
                is_secret: false,
                hint: Some("optional; useful for documenting imported OAuth2 grants"),
            },
            FieldInfo {
                name: "oauth1_consumer_key",
                label: "OAuth1 Consumer Key",
                is_secret: false,
                hint: Some("legacy/fallback only"),
            },
            FieldInfo {
                name: "oauth1_consumer_secret",
                label: "OAuth1 Consumer Secret",
                is_secret: true,
                hint: Some("legacy/fallback only"),
            },
            FieldInfo {
                name: "oauth1_access_token",
                label: "OAuth1 Access Token",
                is_secret: true,
                hint: Some("legacy/fallback only"),
            },
            FieldInfo {
                name: "oauth1_access_token_secret",
                label: "OAuth1 Access Token Secret",
                is_secret: true,
                hint: Some("legacy/fallback only"),
            },
        ],
        instructions: Some(SetupInstructions {
            obtain_url: "https://developer.x.com/",
            steps: &[
                "Create or open an X App, then go to the App's 'Keys and tokens' page",
                "Choose the smallest auth family that fits the job: bearer for public reads, OAuth 2.0 for user-context, OAuth 1.0a only as fallback",
                "For OAuth 2.0, configure your callback URL and complete the consent flow outside rzn-tools, then import the returned token package here",
                "Import only the relevant fields here; blank fields are ignored",
                "After setup, validate with 'rzn-tools x auth-status' and 'rzn-tools x whoami' when using user-context auth",
            ],
        }),
        aliases: &["x-api", "twitter-api", "xapi"],
    },
    ConnectorSetupInfo {
        name: "linkedin",
        display_name: "LinkedIn",
        description: "Official OAuth/OIDC token import for posting, auth status, and raw API requests",
        auth_type: AuthType::MultipleFields,
        env_vars: &[],
        required_fields: &[
            FieldInfo {
                name: "access_token",
                label: "Access Token",
                is_secret: true,
                hint: Some("required; obtained from your external OAuth/OIDC broker"),
            },
            FieldInfo {
                name: "expires_at",
                label: "Access Token Expires At",
                is_secret: false,
                hint: Some("optional; epoch seconds or RFC3339 timestamp"),
            },
            FieldInfo {
                name: "refresh_token",
                label: "Refresh Token",
                is_secret: true,
                hint: Some("optional; only for approved LinkedIn partner refresh flows"),
            },
            FieldInfo {
                name: "refresh_token_expires_at",
                label: "Refresh Token Expires At",
                is_secret: false,
                hint: Some("optional; epoch seconds or RFC3339 timestamp"),
            },
            FieldInfo {
                name: "id_token",
                label: "ID Token",
                is_secret: true,
                hint: Some("optional; OIDC JWT for member identity"),
            },
            FieldInfo {
                name: "scopes",
                label: "Scopes",
                is_secret: false,
                hint: Some("optional; e.g. openid profile email w_member_social"),
            },
            FieldInfo {
                name: "auth_mode",
                label: "Auth Mode",
                is_secret: false,
                hint: Some("optional; member_oauth or app_oauth"),
            },
            FieldInfo {
                name: "organization_urn",
                label: "Organization URN",
                is_secret: false,
                hint: Some("optional; default org for company-share"),
            },
            FieldInfo {
                name: "member_urn",
                label: "Member URN",
                is_secret: false,
                hint: Some("optional; skips userinfo/id_token derivation"),
            },
            FieldInfo {
                name: "client_id",
                label: "Client ID",
                is_secret: false,
                hint: Some("optional; needed for refresh-token calls and full id_token validation"),
            },
            FieldInfo {
                name: "client_secret",
                label: "Client Secret",
                is_secret: true,
                hint: Some("optional; needed for refresh-token calls"),
            },
            FieldInfo {
                name: "linkedin_api_version",
                label: "LinkedIn API Version",
                is_secret: false,
                hint: Some("optional; YYYYMM, defaults to 202603"),
            },
        ],
        instructions: Some(SetupInstructions {
            obtain_url: "https://www.linkedin.com/developers/",
            steps: &[
                "Run the LinkedIn OAuth/OIDC authorization flow outside rzn-tools using your own backend or broker",
                "Capture the returned access_token and any optional refresh_token / id_token metadata",
                "Paste the token package here; rzn-tools will store it but will not launch a browser flow",
                "Use 'rzn-tools linkedin auth-status' to confirm scopes and token state",
            ],
        }),
        aliases: &[],
    },
    // === Browser Cookies ===
    ConnectorSetupInfo {
        name: "x-browser",
        display_name: "X (Browser Cookies)",
        description: "Scraper-based access via browser cookies (threads/conversation context)",
        auth_type: AuthType::BrowserCookies,
        env_vars: &[],
        required_fields: &[FieldInfo {
            name: "browser",
            label: "Browser",
            is_secret: false,
            hint: Some("chrome, firefox, edge, safari, or brave"),
        }],
        instructions: Some(SetupInstructions {
            obtain_url: "https://x.com",
            steps: &[
                "Log in to X (Twitter) in your browser",
                "Make sure you're logged in and can see your timeline",
                "Close the browser completely before running setup",
                "rzn-tools will extract your session cookies automatically",
            ],
        }),
        aliases: &["x-cookies", "twitter-cookies", "twitter-browser", "x-browser-cookies"],
    },
    // === OAuth ===
    ConnectorSetupInfo {
        name: "google-drive",
        display_name: "Google Drive",
        description: "Files and folders",
        auth_type: AuthType::OAuth {
            provider: OAuthProvider::Google {
                scopes: "https://www.googleapis.com/auth/drive.readonly",
            },
        },
        env_vars: &[],
        required_fields: &[],
        instructions: Some(SetupInstructions {
            obtain_url: "https://console.cloud.google.com/apis/credentials",
            steps: &[
                "Create OAuth 2.0 credentials in Google Cloud Console",
                "Or use the device authorization flow below (recommended)",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "google-gmail",
        display_name: "Gmail",
        description: "Email access",
        auth_type: AuthType::OAuth {
            provider: OAuthProvider::Google {
                scopes: "https://www.googleapis.com/auth/gmail.readonly",
            },
        },
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "google-calendar",
        display_name: "Google Calendar",
        description: "Calendar events",
        auth_type: AuthType::OAuth {
            provider: OAuthProvider::Google {
                scopes: "https://www.googleapis.com/auth/calendar.readonly",
            },
        },
        env_vars: &[],
        required_fields: &[],
        instructions: None,
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "google-search-console",
        display_name: "Google Search Console",
        description: "SEO performance, sitemaps, and URL inspection",
        auth_type: AuthType::OAuth {
            provider: OAuthProvider::Google {
                scopes: "https://www.googleapis.com/auth/webmasters",
            },
        },
        env_vars: &[],
        required_fields: &[],
        instructions: Some(SetupInstructions {
            obtain_url:
                "https://github.com/srv1n/rzn-tools/blob/main/docs/connectors/google_search_console.md",
            steps: &[
                "Verify your site/property in Google Search Console (so your account has access)",
                "Enable the Google Search Console API in your Google Cloud project",
                "Create OAuth 2.0 credentials (Desktop app recommended) and copy client_id/client_secret",
                "Use the device authorization flow below to grant access (scope includes URL Inspection)",
            ],
        }),
        aliases: &["gsc", "search-console"],
    },
    ConnectorSetupInfo {
        name: "microsoft-graph",
        display_name: "Microsoft Graph",
        description: "OneDrive, Outlook, Calendar",
        auth_type: AuthType::OAuth {
            provider: OAuthProvider::Microsoft {
                scopes: "Files.Read Mail.Read Calendars.Read User.Read offline_access",
            },
        },
        env_vars: &[],
        required_fields: &[],
        instructions: Some(SetupInstructions {
            obtain_url:
                "https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps/ApplicationsListBlade",
            steps: &[
                "Register an app in Azure AD",
                "Or use the device authorization flow below (recommended)",
            ],
        }),
        aliases: &[],
    },
    ConnectorSetupInfo {
        name: "caldav",
        display_name: "CalDAV",
        description: "Calendar events via CalDAV (iCloud, Fastmail, Nextcloud, Radicale, etc.)",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("CALDAV_BASE_URL", "CalDAV base URL"),
            ("CALDAV_USERNAME", "Username/email"),
            ("CALDAV_PASSWORD", "Password or app-specific password"),
            ("CALDAV_BEARER_TOKEN", "OAuth bearer token (optional alternative)"),
            ("CALDAV_CALENDAR_URL", "Default calendar URL (optional)"),
        ],
        required_fields: &[
            FieldInfo {
                name: "base_url",
                label: "CalDAV Base URL",
                is_secret: false,
                hint: Some("e.g., https://caldav.icloud.com"),
            },
            FieldInfo {
                name: "username",
                label: "Username",
                is_secret: false,
                hint: Some("email/username for Basic auth"),
            },
            FieldInfo {
                name: "password",
                label: "Password",
                is_secret: true,
                hint: Some("app-specific password recommended"),
            },
            FieldInfo {
                name: "bearer_token",
                label: "Bearer Token (optional)",
                is_secret: true,
                hint: Some("use instead of username/password"),
            },
            FieldInfo {
                name: "calendar_url",
                label: "Default Calendar URL (optional)",
                is_secret: false,
                hint: Some("specific calendar collection URL"),
            },
        ],
        instructions: Some(SetupInstructions {
            obtain_url: "https://github.com/srv1n/rzn-tools/blob/main/docs/connectors/caldav.md",
            steps: &[
                "Pick your provider endpoint from the CalDAV guide (iCloud/Fastmail/Nextcloud/Radicale).",
                "For iCloud and many hosted services, generate an app-specific password.",
                "Set base_url + username/password (or bearer token if your server supports OAuth bearer auth).",
                "Run `rzn-tools caldav list-calendars` to discover your calendars and optionally save a default calendar_url.",
            ],
        }),
        aliases: &[],
    },
    // === IMAP Email ===
    ConnectorSetupInfo {
        name: "imap",
        display_name: "IMAP Email",
        description: "Access emails via IMAP (Gmail, iCloud, Outlook, Yahoo, and more)",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("IMAP_HOST", "Server hostname"),
            ("IMAP_PORT", "Port (usually 993)"),
            ("IMAP_USERNAME", "Email address"),
            ("IMAP_PASSWORD", "Password or App Password"),
        ],
        required_fields: &[
            FieldInfo {
                name: "host",
                label: "IMAP Server",
                is_secret: false,
                hint: Some("e.g., imap.gmail.com"),
            },
            FieldInfo {
                name: "port",
                label: "Port",
                is_secret: false,
                hint: Some("usually 993 for SSL"),
            },
            FieldInfo {
                name: "username",
                label: "Email Address",
                is_secret: false,
                hint: None,
            },
            FieldInfo {
                name: "password",
                label: "Password",
                is_secret: true,
                hint: Some("App Password recommended"),
            },
        ],
        instructions: None, // We'll handle this specially with provider selection
        aliases: &["email"],
    },
    // === SMTP Email ===
    ConnectorSetupInfo {
        name: "smtp",
        display_name: "SMTP Email",
        description: "Send outbound emails via SMTP (Gmail, iCloud, Outlook, Yahoo, and more)",
        auth_type: AuthType::MultipleFields,
        env_vars: &[
            ("SMTP_HOST", "Server hostname"),
            ("SMTP_PORT", "Port (usually 587 for STARTTLS)"),
            ("SMTP_USERNAME", "Email address"),
            ("SMTP_PASSWORD", "Password or App Password"),
            ("SMTP_SECURITY", "Security mode: starttls, tls, or plaintext"),
            ("SMTP_FROM_ADDRESS", "Default sender address (optional)"),
            ("SMTP_FROM_NAME", "Default sender display name (optional)"),
        ],
        required_fields: &[
            FieldInfo {
                name: "host",
                label: "SMTP Server",
                is_secret: false,
                hint: Some("e.g., smtp.gmail.com"),
            },
            FieldInfo {
                name: "port",
                label: "Port",
                is_secret: false,
                hint: Some("587 (STARTTLS), 465 (TLS), 25 (plaintext)"),
            },
            FieldInfo {
                name: "username",
                label: "Email Address",
                is_secret: false,
                hint: None,
            },
            FieldInfo {
                name: "password",
                label: "Password",
                is_secret: true,
                hint: Some("App Password recommended"),
            },
            FieldInfo {
                name: "security",
                label: "Security",
                is_secret: false,
                hint: Some("starttls, tls, or plaintext"),
            },
            FieldInfo {
                name: "from_address",
                label: "Default From Address (optional)",
                is_secret: false,
                hint: Some("defaults to username"),
            },
            FieldInfo {
                name: "from_name",
                label: "Default From Name (optional)",
                is_secret: false,
                hint: Some("e.g., Team Notifications"),
            },
        ],
        instructions: None, // We'll handle this specially with provider selection
        aliases: &["mailer"],
    },
];

/// IMAP provider presets for easy configuration
struct ImapProvider {
    name: &'static str,
    display_name: &'static str,
    host: &'static str,
    port: u16,
    help_url: &'static str,
    app_password_steps: &'static [&'static str],
}

const IMAP_PROVIDERS: &[ImapProvider] = &[
    ImapProvider {
        name: "gmail",
        display_name: "Gmail (Google)",
        host: "imap.gmail.com",
        port: 993,
        help_url: "https://support.google.com/mail/answer/7126229",
        app_password_steps: &[
            "Go to https://myaccount.google.com/apppasswords",
            "Sign in with your Google account",
            "Select 'Mail' as the app and your device type",
            "Click 'Generate' and copy the 16-character password",
            "Use this App Password instead of your regular password",
        ],
    },
    ImapProvider {
        name: "icloud",
        display_name: "iCloud Mail (Apple)",
        host: "imap.mail.me.com",
        port: 993,
        help_url: "https://support.apple.com/en-us/102525",
        app_password_steps: &[
            "Go to https://appleid.apple.com/account/manage",
            "Sign in with your Apple ID",
            "In the 'Sign-In and Security' section, click 'App-Specific Passwords'",
            "Click '+' to generate a new password",
            "Enter a label (e.g., 'rzn-tools') and click 'Create'",
            "Copy the generated password and use it below",
        ],
    },
    ImapProvider {
        name: "outlook",
        display_name: "Outlook.com / Hotmail / Live",
        host: "imap-mail.outlook.com",
        port: 993,
        help_url: "https://support.microsoft.com/en-us/office/pop-imap-and-smtp-settings-for-outlook-com-d088b986-291d-42b8-9564-9c414e2aa040",
        app_password_steps: &[
            "Go to https://account.live.com/proofs/manage/additional",
            "Sign in and go to Security > Advanced security options",
            "Under 'App passwords', click 'Create a new app password'",
            "Copy the generated password and use it below",
            "Note: You must have 2-step verification enabled",
        ],
    },
    ImapProvider {
        name: "office365",
        display_name: "Microsoft 365 (Work/School)",
        host: "outlook.office365.com",
        port: 993,
        help_url: "https://learn.microsoft.com/en-us/exchange/clients-and-mobile-in-exchange-online/pop3-and-imap4/pop3-and-imap4",
        app_password_steps: &[
            "Note: Your admin must enable IMAP access for your account",
            "Go to https://mysignins.microsoft.com/security-info",
            "Add a new sign-in method and select 'App password'",
            "Copy the generated password and use it below",
            "Contact your IT admin if IMAP access is blocked",
        ],
    },
    ImapProvider {
        name: "yahoo",
        display_name: "Yahoo Mail",
        host: "imap.mail.yahoo.com",
        port: 993,
        help_url: "https://help.yahoo.com/kb/SLN4075.html",
        app_password_steps: &[
            "Go to https://login.yahoo.com/myaccount/security/",
            "Sign in and scroll to 'App password' (requires 2-step verification)",
            "Click 'Generate app password'",
            "Select 'Other App' and enter 'rzn-tools'",
            "Copy the generated password and use it below",
        ],
    },
    ImapProvider {
        name: "fastmail",
        display_name: "Fastmail",
        host: "imap.fastmail.com",
        port: 993,
        help_url: "https://www.fastmail.help/hc/en-us/articles/360058753834",
        app_password_steps: &[
            "Go to https://www.fastmail.com/settings/security/devicekeys",
            "Click 'New App Password'",
            "Enter a name (e.g., 'rzn-tools') and select 'IMAP' access",
            "Click 'Generate Password'",
            "Copy the password and use it below",
        ],
    },
    ImapProvider {
        name: "protonmail",
        display_name: "ProtonMail (via Bridge)",
        host: "127.0.0.1",
        port: 1143,
        help_url: "https://proton.me/support/protonmail-bridge-install",
        app_password_steps: &[
            "Install and run ProtonMail Bridge from https://proton.me/mail/bridge",
            "Sign in to Bridge with your ProtonMail account",
            "In Bridge, click your account to see IMAP credentials",
            "Use the Bridge password shown (not your ProtonMail password)",
            "Note: Bridge must be running for IMAP access to work",
        ],
    },
];

/// SMTP provider presets for easy configuration
struct SmtpProvider {
    name: &'static str,
    display_name: &'static str,
    host: &'static str,
    port: u16,
    security: &'static str,
    help_url: &'static str,
    app_password_steps: &'static [&'static str],
}

const SMTP_PROVIDERS: &[SmtpProvider] = &[
    SmtpProvider {
        name: "gmail",
        display_name: "Gmail (Google)",
        host: "smtp.gmail.com",
        port: 587,
        security: "starttls",
        help_url: "https://support.google.com/mail/answer/7126229",
        app_password_steps: &[
            "Go to https://myaccount.google.com/apppasswords",
            "Sign in with your Google account",
            "Select 'Mail' as the app and your device type",
            "Click 'Generate' and copy the 16-character password",
            "Use this App Password instead of your regular password",
        ],
    },
    SmtpProvider {
        name: "icloud",
        display_name: "iCloud Mail (Apple)",
        host: "smtp.mail.me.com",
        port: 587,
        security: "starttls",
        help_url: "https://support.apple.com/en-us/102525",
        app_password_steps: &[
            "Go to https://appleid.apple.com/account/manage",
            "Sign in with your Apple ID",
            "In the 'Sign-In and Security' section, click 'App-Specific Passwords'",
            "Click '+' to generate a new password",
            "Enter a label (e.g., 'rzn-tools') and click 'Create'",
            "Copy the generated password and use it below",
        ],
    },
    SmtpProvider {
        name: "outlook",
        display_name: "Outlook.com / Hotmail / Live",
        host: "smtp-mail.outlook.com",
        port: 587,
        security: "starttls",
        help_url: "https://support.microsoft.com/en-us/office/pop-imap-and-smtp-settings-for-outlook-com-d088b986-291d-42b8-9564-9c414e2aa040",
        app_password_steps: &[
            "Go to https://account.live.com/proofs/manage/additional",
            "Sign in and go to Security > Advanced security options",
            "Under 'App passwords', click 'Create a new app password'",
            "Copy the generated password and use it below",
            "Note: You must have 2-step verification enabled",
        ],
    },
    SmtpProvider {
        name: "office365",
        display_name: "Microsoft 365 (Work/School)",
        host: "smtp.office365.com",
        port: 587,
        security: "starttls",
        help_url: "https://learn.microsoft.com/en-us/exchange/clients-and-mobile-in-exchange-online/how-to-set-up-a-multifunction-device-or-application-to-send-email-using-microsoft-365-or-office-365",
        app_password_steps: &[
            "Note: Your admin must enable SMTP AUTH for your account",
            "Go to https://mysignins.microsoft.com/security-info",
            "Add a new sign-in method and select 'App password'",
            "Copy the generated password and use it below",
            "Contact your IT admin if SMTP AUTH is blocked",
        ],
    },
    SmtpProvider {
        name: "yahoo",
        display_name: "Yahoo Mail",
        host: "smtp.mail.yahoo.com",
        port: 465,
        security: "tls",
        help_url: "https://help.yahoo.com/kb/SLN4075.html",
        app_password_steps: &[
            "Go to https://login.yahoo.com/myaccount/security/",
            "Sign in and scroll to 'App password' (requires 2-step verification)",
            "Click 'Generate app password'",
            "Select 'Other App' and enter 'rzn-tools'",
            "Copy the generated password and use it below",
        ],
    },
    SmtpProvider {
        name: "fastmail",
        display_name: "Fastmail",
        host: "smtp.fastmail.com",
        port: 465,
        security: "tls",
        help_url: "https://www.fastmail.help/hc/en-us/articles/1500000278102",
        app_password_steps: &[
            "Go to https://www.fastmail.com/settings/security/devicekeys",
            "Click 'New App Password'",
            "Enter a name (e.g., 'rzn-tools') and select 'SMTP' access",
            "Click 'Generate Password'",
            "Copy the password and use it below",
        ],
    },
    SmtpProvider {
        name: "protonmail",
        display_name: "ProtonMail (via Bridge)",
        host: "127.0.0.1",
        port: 1025,
        security: "plaintext",
        help_url: "https://proton.me/support/protonmail-bridge-install",
        app_password_steps: &[
            "Install and run ProtonMail Bridge from https://proton.me/mail/bridge",
            "Sign in to Bridge with your ProtonMail account",
            "In Bridge, open account settings to see SMTP credentials",
            "Use the Bridge password shown (not your ProtonMail password)",
            "Note: Bridge must be running for SMTP access to work",
        ],
    },
];

pub async fn run(cli: &Cli, connector: Option<&str>) -> Result<()> {
    if let Some(connector_name) = connector {
        setup_connector(cli, connector_name).await
    } else {
        run_setup_wizard(cli).await
    }
}

fn selected_auth_profile(cli: &Cli) -> &str {
    cli.auth_profile.as_deref().unwrap_or("default")
}

async fn run_setup_wizard(cli: &Cli) -> Result<()> {
    println!();
    println!("{}", "rzn-tools Setup".bold().cyan());
    println!("{}", "===========".cyan());
    println!();
    if selected_auth_profile(cli) != "default" {
        println!(
            "{} {}",
            "Auth profile:".bold(),
            selected_auth_profile(cli).cyan()
        );
        println!();
    }
    println!("Configure connectors for accessing external data sources.");
    println!();

    // Show available connectors grouped by auth requirement
    println!("{}", "Available Connectors:".bold());
    println!();

    // No auth required
    println!(
        "  {} (no authentication required):",
        "Ready to use".green().bold()
    );
    for info in CONNECTORS
        .iter()
        .filter(|c| matches!(c.auth_type, AuthType::None))
    {
        println!(
            "    {} - {}",
            info.display_name.cyan(),
            info.description.dimmed()
        );
    }
    println!();

    // Auth required
    println!(
        "  {} (authentication required):",
        "Needs setup".yellow().bold()
    );
    for info in CONNECTORS
        .iter()
        .filter(|c| !matches!(c.auth_type, AuthType::None))
    {
        let auth_hint =
            match info.auth_type {
                AuthType::ApiKey => {
                    if info.required_fields.iter().any(|f| {
                        f.name == "bearer_token" || f.label.to_lowercase().contains("bearer")
                    }) {
                        "[Bearer Token]"
                    } else {
                        "[API Key]"
                    }
                }
                AuthType::OAuth { .. } => "[OAuth]",
                AuthType::BrowserCookies => "[Browser Cookies]",
                AuthType::MultipleFields => "[Credentials]",
                AuthType::None => "",
            };
        println!(
            "    {} {} - {}",
            info.display_name.cyan(),
            auth_hint.dimmed(),
            info.description.dimmed()
        );
    }
    println!();

    // Ask which connector to configure
    println!("{}", "Which connector would you like to configure?".bold());
    println!("Enter connector name (e.g., 'slack', 'github') or 'q' to quit:");
    print!("{} ", ">".green().bold());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let connector_name = input.trim().to_lowercase();

    if connector_name == "q" || connector_name == "quit" || connector_name.is_empty() {
        println!();
        println!(
            "{}",
            "Run 'rzn-tools setup <connector>' anytime to configure a connector.".green()
        );
        return Ok(());
    }

    // Find the connector
    let info = CONNECTORS.iter().find(|c| {
        c.name == connector_name.as_str() || c.aliases.contains(&connector_name.as_str())
    });

    if let Some(info) = info {
        configure_connector(cli, info).await?;
    } else {
        println!();
        println!(
            "{} Unknown connector '{}'. Available connectors:",
            "Error:".red().bold(),
            connector_name
        );
        for info in CONNECTORS {
            println!("  - {}", info.name);
        }
    }

    Ok(())
}

async fn setup_connector(cli: &Cli, connector_name: &str) -> Result<()> {
    let info = CONNECTORS
        .iter()
        .find(|c| c.name == connector_name || c.aliases.contains(&connector_name));

    if let Some(info) = info {
        println!();
        println!(
            "{} {}",
            "Setting up".bold().cyan(),
            info.display_name.bold()
        );
        println!("{}", info.description.dimmed());
        println!();

        configure_connector(cli, info).await?;
    } else {
        // Try to show tools for the connector even if not in our predefined list
        let registry = crate::commands::list::create_registry(cli.auth_profile.as_deref()).await?;

        if let Some(provider) = registry.get_provider(connector_name) {
            let c = provider.lock().await;
            let tools_response = c
                .list_tools(Some(PaginatedRequestParam { cursor: None }))
                .await?;

            println!();
            println!("{} {}", "Connector:".bold().cyan(), connector_name.yellow());
            println!();

            if tools_response.tools.is_empty() {
                println!("{}", "No tools available for this connector.".yellow());
            } else {
                println!("{}", "Available tools:".bold().green());
                for tool in &tools_response.tools {
                    println!(
                        "  {} - {}",
                        tool.name.cyan().bold(),
                        tool.description
                            .as_deref()
                            .unwrap_or("No description")
                            .dimmed()
                    );
                }
            }

            // Show config schema if available
            let schema = c.config_schema();
            if !schema.fields.is_empty() {
                println!();
                println!("{}", "Required configuration:".bold().yellow());
                for field in &schema.fields {
                    let req = if field.required {
                        "(required)"
                    } else {
                        "(optional)"
                    };
                    println!(
                        "  {} {} - {}",
                        field.name.cyan(),
                        req.dimmed(),
                        field.description.as_deref().unwrap_or("").dimmed()
                    );
                }
                println!();
                println!("To configure, run:");
                println!("  {}", format!("rzn-tools setup {}", connector_name).cyan());
            } else {
                println!();
                println!("{}", "This connector requires no authentication.".green());
            }
        } else {
            return Err(CommandError::ConnectorNotFound(connector_name.to_string()));
        }
    }

    Ok(())
}

async fn configure_connector(cli: &Cli, info: &ConnectorSetupInfo) -> Result<()> {
    let auth_profile = selected_auth_profile(cli);

    // Special handling for IMAP with provider selection
    if info.name == "imap" {
        return configure_imap(cli).await;
    }
    if info.name == "smtp" {
        return configure_smtp(cli).await;
    }

    match info.auth_type {
        AuthType::None => {
            println!(
                "{} {} requires no authentication.",
                "Ready!".green().bold(),
                info.display_name
            );
            println!();
            println!("{}", "Try it now:".bold());
            println!(
                "  {}",
                format!("rzn-tools search {} \"your query\"", info.name).cyan()
            );
        }
        AuthType::ApiKey | AuthType::MultipleFields => {
            // Show instructions if available
            if let Some(instructions) = &info.instructions {
                println!("{}", "How to get credentials:".bold());
                println!("  {}", instructions.obtain_url.cyan().underline());
                println!();
                for (i, step) in instructions.steps.iter().enumerate() {
                    println!("  {}. {}", i + 1, step);
                }
                println!();
            }

            println!("{}", "Configuration options:".bold());
            println!();

            // Show environment variable option
            if !info.env_vars.is_empty() {
                println!(
                    "  {} Set environment variables:",
                    "Option 1:".yellow().bold()
                );
                for (env_var, desc) in info.env_vars {
                    println!("    export {}=\"<{}>\"", env_var, desc);
                }
                println!();
            }

            // Show interactive option
            let store = FileAuthStore::new_default();
            let config_path = store.config_path();
            println!(
                "  {} Enter credentials now (stored in {}):",
                "Option 2:".yellow().bold(),
                config_path.dimmed()
            );
            if auth_profile != "default" {
                println!(
                    "      {}",
                    format!("auth profile: {}", auth_profile).dimmed()
                );
            }
            println!();

            print!("Enter credentials now? [y/N] ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

            if input.trim().to_lowercase() == "y" {
                let mut auth = AuthDetails::new();

                for field in info.required_fields {
                    let hint = field.hint.map(|h| format!(" ({})", h)).unwrap_or_default();
                    print!("  {}{}: ", field.label.bold(), hint.dimmed());
                    io::stdout().flush()?;

                    let value = if field.is_secret {
                        read_secret()?
                    } else {
                        let mut v = String::new();
                        io::stdin().read_line(&mut v)?;
                        v.trim().to_string()
                    };

                    if !value.is_empty() {
                        auth.insert(field.name.to_string(), value);
                    }
                }

                if !auth.is_empty() {
                    // Save credentials
                    store
                        .save_profile(info.name, auth_profile, &auth)
                        .map_err(|e| {
                            CommandError::InvalidConfig(format!(
                                "Failed to save credentials: {}",
                                e
                            ))
                        })?;

                    println!();
                    println!(
                        "{} Credentials saved for {}",
                        "Saved!".green().bold(),
                        info.display_name
                    );

                    // Test the connection
                    println!();
                    print!("{}", "Testing connection... ".dimmed());
                    io::stdout().flush()?;

                    match test_connector_auth(cli, info.name).await {
                        Ok(_) => {
                            println!("{}", "Success!".green().bold());
                            println!();
                            println!("{}", "You're all set! Try:".bold());
                            let example = if info.name == "bing-webmaster-tools" {
                                format!("rzn-tools {} list-sites", info.name)
                            } else if info.name == "x" {
                                "rzn-tools x auth-status".to_string()
                            } else if info.name == "linkedin" {
                                "rzn-tools linkedin auth-status".to_string()
                            } else {
                                format!("rzn-tools search {} \"test query\"", info.name)
                            };
                            println!("  {}", example.cyan());
                        }
                        Err(e) => {
                            println!("{}", "Failed".red().bold());
                            println!();
                            println!("{} {}", "Error:".red().bold(), e.to_string().red());
                            println!();
                            println!("Your credentials were saved. You can:");
                            println!("  - Check if the credentials are correct");
                            println!(
                                "  - Re-run {} to try again",
                                format!("rzn-tools setup {}", info.name).cyan()
                            );
                            println!(
                                "  - Test manually with {}",
                                format!("rzn-tools config test {}", info.name).cyan()
                            );
                        }
                    }
                }
            } else {
                show_later_instructions(info);
            }
        }
        AuthType::BrowserCookies => {
            // Show instructions
            if let Some(instructions) = &info.instructions {
                println!("{}", "Prerequisites:".bold());
                for (i, step) in instructions.steps.iter().enumerate() {
                    println!("  {}. {}", i + 1, step);
                }
                println!();
            }

            println!("{}", "Supported browsers:".bold());
            println!("  - Chrome");
            println!("  - Firefox");
            println!("  - Edge");
            println!("  - Safari (macOS only)");
            println!("  - Brave");
            println!();

            print!("Which browser are you logged into? [chrome/firefox/edge/safari/brave]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let browser = input.trim().to_lowercase();

            if ["chrome", "firefox", "edge", "safari", "brave"].contains(&browser.as_str()) {
                let mut auth = AuthDetails::new();
                auth.insert("browser".to_string(), browser.clone());

                let store = FileAuthStore::new_default();
                store
                    .save_profile(info.name, auth_profile, &auth)
                    .map_err(|e| {
                        CommandError::InvalidConfig(format!("Failed to save config: {}", e))
                    })?;

                println!();
                println!(
                    "{} Browser set to {}",
                    "Saved!".green().bold(),
                    browser.cyan()
                );

                // Test the connection
                println!();
                print!("{}", "Extracting cookies and testing... ".dimmed());
                io::stdout().flush()?;

                match test_connector_auth(cli, info.name).await {
                    Ok(_) => {
                        println!("{}", "Success!".green().bold());
                        println!();
                        println!("{}", "You're all set! Try:".bold());
                        println!(
                            "  {}",
                            format!("rzn-tools {} search_tweets \"rust\"", info.name).cyan()
                        );
                    }
                    Err(e) => {
                        println!("{}", "Failed".red().bold());
                        println!();
                        println!("{} {}", "Error:".red().bold(), e.to_string().red());
                        println!();
                        println!("Make sure you:");
                        println!("  - Are logged into {} in {}", info.display_name, browser);
                        println!("  - Have closed the browser completely");
                        println!("  - Have granted rzn-tools permission to access cookies (macOS)");
                    }
                }
            } else if !browser.is_empty() {
                println!();
                println!(
                    "{} '{}' is not supported. Use: chrome, firefox, edge, safari, or brave",
                    "Error:".red().bold(),
                    browser
                );
            }
        }
        AuthType::OAuth { provider } => {
            configure_oauth(cli, info, provider).await?;
        }
    }

    Ok(())
}

async fn configure_imap(cli: &Cli) -> Result<()> {
    let auth_profile = selected_auth_profile(cli);

    println!("{}", "IMAP Email Setup".bold().cyan());
    println!();
    println!("Select your email provider for automatic server configuration,");
    println!("or choose 'Other' to enter settings manually.");
    println!();

    // Show provider options
    println!("{}", "Email Providers:".bold());
    for (i, provider) in IMAP_PROVIDERS.iter().enumerate() {
        println!("  {}. {}", i + 1, provider.display_name);
    }
    println!(
        "  {}. Other (manual configuration)",
        IMAP_PROVIDERS.len() + 1
    );
    println!();

    print!("Select provider [1-{}]: ", IMAP_PROVIDERS.len() + 1);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let selection: usize = input.trim().parse().unwrap_or(0);

    let (host, port, provider_name) = if selection > 0 && selection <= IMAP_PROVIDERS.len() {
        let provider = &IMAP_PROVIDERS[selection - 1];

        println!();
        println!("{}", "━".repeat(60).dimmed());
        println!();
        println!(
            "{} {}",
            "Setting up:".bold().green(),
            provider.display_name.cyan()
        );
        println!();

        // Show help URL
        println!("{}", "📖 Official documentation:".bold());
        println!("   {}", provider.help_url.cyan().underline());
        println!();

        // Show app password instructions
        println!("{}", "🔑 How to get an App Password:".bold());
        for (i, step) in provider.app_password_steps.iter().enumerate() {
            println!("   {}. {}", i + 1, step);
        }
        println!();
        println!("{}", "━".repeat(60).dimmed());
        println!();

        println!(
            "{} Server: {} | Port: {}",
            "Auto-configured:".green().bold(),
            provider.host.cyan(),
            provider.port.to_string().cyan()
        );
        println!();

        (
            provider.host.to_string(),
            provider.port.to_string(),
            Some(provider.name),
        )
    } else if selection == IMAP_PROVIDERS.len() + 1 {
        // Manual configuration
        println!();
        println!("{}", "Manual IMAP Configuration".bold());
        println!();

        print!("  {} (e.g., imap.example.com): ", "IMAP Server".bold());
        io::stdout().flush()?;
        let mut host = String::new();
        io::stdin().read_line(&mut host)?;
        let host = host.trim().to_string();

        print!("  {} (usually 993 for SSL): ", "Port".bold());
        io::stdout().flush()?;
        let mut port = String::new();
        io::stdin().read_line(&mut port)?;
        let port = port.trim().to_string();
        let port = if port.is_empty() {
            "993".to_string()
        } else {
            port
        };

        (host, port, None)
    } else {
        println!("{}", "Invalid selection. Please try again.".red());
        return Ok(());
    };

    if host.is_empty() {
        println!("{}", "Server hostname is required.".red());
        return Ok(());
    }

    // Get email address
    print!("  {}: ", "Email Address".bold());
    io::stdout().flush()?;
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;
    let username = username.trim().to_string();

    if username.is_empty() {
        println!("{}", "Email address is required.".red());
        return Ok(());
    }

    // Get password
    print!("  {} (App Password recommended): ", "Password".bold());
    io::stdout().flush()?;
    let password = read_secret()?;

    if password.is_empty() {
        println!("{}", "Password is required.".red());
        return Ok(());
    }

    // Save credentials
    let store = FileAuthStore::new_default();
    let mut auth = AuthDetails::new();
    auth.insert("host".to_string(), host.clone());
    auth.insert("port".to_string(), port.clone());
    auth.insert("username".to_string(), username.clone());
    auth.insert("password".to_string(), password);
    auth.insert("security".to_string(), "tls".to_string());

    // Store provider hint for potential future use
    if let Some(prov) = provider_name {
        auth.insert("provider".to_string(), prov.to_string());
    }

    store
        .save_profile("imap", auth_profile, &auth)
        .map_err(|e| CommandError::InvalidConfig(format!("Failed to save credentials: {}", e)))?;

    println!();
    println!(
        "{} IMAP credentials saved for {}",
        "Saved!".green().bold(),
        username.cyan()
    );

    // Test the connection
    println!();
    print!("{}", "Testing connection... ".dimmed());
    io::stdout().flush()?;

    match test_connector_auth(cli, "imap").await {
        Ok(_) => {
            println!("{}", "Success!".green().bold());
            println!();
            println!("{}", "You're all set! Try:".bold());
            println!("  {}", "rzn-tools imap list_mailboxes".cyan());
            println!(
                "  {}",
                "rzn-tools imap search_emails \"from:someone@example.com\"".cyan()
            );
        }
        Err(e) => {
            println!("{}", "Failed".red().bold());
            println!();
            println!("{} {}", "Error:".red().bold(), e.to_string().red());
            println!();
            println!("Troubleshooting tips:");
            println!(
                "  • Make sure you're using an {} (not your regular password)",
                "App Password".bold()
            );
            println!("  • Check that IMAP is enabled in your email settings");
            println!(
                "  • Verify the server ({}) and port ({}) are correct",
                host.cyan(),
                port.cyan()
            );
            if provider_name.is_some() {
                println!("  • Visit the documentation link above for provider-specific help");
            }
            println!();
            println!("Re-run {} to try again.", "rzn-tools setup imap".cyan());
        }
    }

    Ok(())
}

async fn configure_smtp(cli: &Cli) -> Result<()> {
    let auth_profile = selected_auth_profile(cli);

    println!("{}", "SMTP Email Setup".bold().cyan());
    println!();
    println!("Select your email provider for automatic SMTP configuration,");
    println!("or choose 'Other' to enter settings manually.");
    println!();

    println!("{}", "Email Providers:".bold());
    for (i, provider) in SMTP_PROVIDERS.iter().enumerate() {
        println!("  {}. {}", i + 1, provider.display_name);
    }
    println!(
        "  {}. Other (manual configuration)",
        SMTP_PROVIDERS.len() + 1
    );
    println!();

    print!("Select provider [1-{}]: ", SMTP_PROVIDERS.len() + 1);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let selection: usize = input.trim().parse().unwrap_or(0);

    let (host, port, security, provider_name) =
        if selection > 0 && selection <= SMTP_PROVIDERS.len() {
            let provider = &SMTP_PROVIDERS[selection - 1];

            println!();
            println!("{}", "━".repeat(60).dimmed());
            println!();
            println!(
                "{} {}",
                "Setting up:".bold().green(),
                provider.display_name.cyan()
            );
            println!();

            println!("{}", "📖 Official documentation:".bold());
            println!("   {}", provider.help_url.cyan().underline());
            println!();

            println!("{}", "🔑 How to get an App Password:".bold());
            for (i, step) in provider.app_password_steps.iter().enumerate() {
                println!("   {}. {}", i + 1, step);
            }
            println!();
            println!("{}", "━".repeat(60).dimmed());
            println!();

            println!(
                "{} Server: {} | Port: {} | Security: {}",
                "Auto-configured:".green().bold(),
                provider.host.cyan(),
                provider.port.to_string().cyan(),
                provider.security.cyan()
            );
            println!();

            (
                provider.host.to_string(),
                provider.port.to_string(),
                provider.security.to_string(),
                Some(provider.name),
            )
        } else if selection == SMTP_PROVIDERS.len() + 1 {
            println!();
            println!("{}", "Manual SMTP Configuration".bold());
            println!();

            print!("  {} (e.g., smtp.example.com): ", "SMTP Server".bold());
            io::stdout().flush()?;
            let mut host = String::new();
            io::stdin().read_line(&mut host)?;
            let host = host.trim().to_string();

            print!("  {} (587 recommended): ", "Port".bold());
            io::stdout().flush()?;
            let mut port = String::new();
            io::stdin().read_line(&mut port)?;
            let port = port.trim().to_string();
            let port = if port.is_empty() {
                "587".to_string()
            } else {
                port
            };

            println!("  {}:", "Security".bold());
            println!("    1) starttls (recommended)");
            println!("    2) tls (SMTPS / implicit TLS)");
            println!("    3) plaintext (local relay only)");
            print!("  Select security [1-3, default 1]: ");
            io::stdout().flush()?;
            let mut security_input = String::new();
            io::stdin().read_line(&mut security_input)?;
            let security = match security_input.trim() {
                "2" | "tls" => "tls",
                "3" | "plaintext" | "plain" => "plaintext",
                _ => "starttls",
            };

            (host, port, security.to_string(), None)
        } else {
            println!("{}", "Invalid selection. Please try again.".red());
            return Ok(());
        };

    if host.is_empty() {
        println!("{}", "Server hostname is required.".red());
        return Ok(());
    }

    print!("  {}: ", "Email Address".bold());
    io::stdout().flush()?;
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;
    let username = username.trim().to_string();

    if username.is_empty() {
        println!("{}", "Email address is required.".red());
        return Ok(());
    }

    print!("  {} (App Password recommended): ", "Password".bold());
    io::stdout().flush()?;
    let password = read_secret()?;

    if password.is_empty() {
        println!("{}", "Password is required.".red());
        return Ok(());
    }

    print!(
        "  {} (press Enter to use username): ",
        "Default From Address".bold()
    );
    io::stdout().flush()?;
    let mut from_address = String::new();
    io::stdin().read_line(&mut from_address)?;
    let from_address = from_address.trim().to_string();

    print!("  {} (optional): ", "Default From Name".bold());
    io::stdout().flush()?;
    let mut from_name = String::new();
    io::stdin().read_line(&mut from_name)?;
    let from_name = from_name.trim().to_string();

    let store = FileAuthStore::new_default();
    let mut auth = AuthDetails::new();
    auth.insert("host".to_string(), host.clone());
    auth.insert("port".to_string(), port.clone());
    auth.insert("username".to_string(), username.clone());
    auth.insert("password".to_string(), password);
    auth.insert("security".to_string(), security.clone());

    if !from_address.is_empty() {
        auth.insert("from_address".to_string(), from_address);
    }
    if !from_name.is_empty() {
        auth.insert("from_name".to_string(), from_name);
    }

    let selected_known_provider = provider_name.is_some();
    if let Some(provider) = provider_name {
        auth.insert("provider".to_string(), provider.to_string());
    }

    store
        .save_profile("smtp", auth_profile, &auth)
        .map_err(|e| CommandError::InvalidConfig(format!("Failed to save credentials: {}", e)))?;

    println!();
    println!(
        "{} SMTP credentials saved for {}",
        "Saved!".green().bold(),
        username.cyan()
    );

    println!();
    print!("{}", "Testing connection... ".dimmed());
    io::stdout().flush()?;

    match test_connector_auth(cli, "smtp").await {
        Ok(_) => {
            println!("{}", "Success!".green().bold());
            println!();
            println!("{}", "You're all set! Try:".bold());
            println!("  {}", "rzn-tools smtp test-connection".cyan());
            println!(
                "  {}",
                "rzn-tools smtp send-mail --to user@example.com --subject \"Hello\" --body \"Test\""
                    .cyan()
            );
        }
        Err(e) => {
            println!("{}", "Failed".red().bold());
            println!();
            println!("{} {}", "Error:".red().bold(), e.to_string().red());
            println!();
            println!("Troubleshooting tips:");
            println!(
                "  • Make sure you're using an {} (not your regular password)",
                "App Password".bold()
            );
            println!("  • Verify SMTP server, port, and security mode");
            println!("  • Confirm your provider allows SMTP AUTH for this account");
            if selected_known_provider {
                println!("  • Visit the documentation link above for provider-specific help");
            }
            println!();
            println!("Re-run {} to try again.", "rzn-tools setup smtp".cyan());
        }
    }

    Ok(())
}

async fn configure_oauth(
    cli: &Cli,
    info: &ConnectorSetupInfo,
    provider: OAuthProvider,
) -> Result<()> {
    let auth_profile = selected_auth_profile(cli);

    println!("{}", "OAuth Authorization".bold());
    println!();
    println!(
        "This will open a device authorization flow. You'll get a code to enter in your browser."
    );
    println!();

    print!("Start authorization? [Y/n] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    if input.trim().to_lowercase() == "n" {
        show_later_instructions(info);
        return Ok(());
    }

    // Check if user has custom client credentials
    println!();
    print!("Do you have your own OAuth client credentials? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let (client_id, client_secret) = if input.trim().to_lowercase() == "y" {
        print!("  Client ID: ");
        io::stdout().flush()?;
        let mut cid = String::new();
        io::stdin().read_line(&mut cid)?;

        print!("  Client Secret (optional, press Enter to skip): ");
        io::stdout().flush()?;
        let cs = read_secret()?;

        (
            cid.trim().to_string(),
            if cs.is_empty() { None } else { Some(cs) },
        )
    } else {
        // Use default public client (would need to be configured per-app)
        match provider {
            OAuthProvider::Google { .. } => {
                println!();
                println!(
                    "{} You need to provide OAuth client credentials for Google.",
                    "Note:".yellow().bold()
                );
                println!(
                    "Get them from: {}",
                    "https://console.cloud.google.com/apis/credentials"
                        .cyan()
                        .underline()
                );
                return Ok(());
            }
            OAuthProvider::Microsoft { .. } => {
                println!();
                println!(
                    "{} You need to provide OAuth client credentials for Microsoft.",
                    "Note:".yellow().bold()
                );
                println!(
                    "Get them from: {}",
                    "https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps"
                        .cyan()
                        .underline()
                );
                return Ok(());
            }
        }
    };

    println!();
    print!("{}", "Starting device authorization... ".dimmed());
    io::stdout().flush()?;

    let device_auth = match provider {
        OAuthProvider::Google { scopes } => google_device_authorize(&client_id, scopes).await?,
        OAuthProvider::Microsoft { scopes } => {
            ms_device_authorize("common", &client_id, scopes).await?
        }
    };

    println!("{}", "Done!".green());
    println!();
    println!("{}", "=".repeat(50).dimmed());
    println!();
    println!(
        "  Go to: {}",
        device_auth.verification_uri.cyan().bold().underline()
    );
    println!("  Enter code: {}", device_auth.user_code.yellow().bold());
    println!();
    println!("{}", "=".repeat(50).dimmed());
    println!();
    println!("{}", "Waiting for authorization...".dimmed());
    println!("(Press Ctrl+C to cancel)");
    println!();

    // Poll for token
    let interval = device_auth.interval.unwrap_or(5) as u64;
    let max_attempts = (device_auth.expires_in as u64) / interval;

    for attempt in 0..max_attempts {
        tokio::time::sleep(tokio::time::Duration::from_secs(interval)).await;

        let result = match provider {
            OAuthProvider::Google { .. } => {
                google_device_poll(
                    &client_id,
                    client_secret.as_deref(),
                    &device_auth.device_code,
                )
                .await
            }
            OAuthProvider::Microsoft { .. } => {
                ms_device_poll("common", &client_id, &device_auth.device_code).await
            }
        };

        match result {
            Ok(tokens) => {
                // Save tokens
                let mut auth = AuthDetails::new();
                auth.insert("access_token".to_string(), tokens.access_token);
                if let Some(rt) = tokens.refresh_token {
                    auth.insert("refresh_token".to_string(), rt);
                }
                if let Some(exp) = tokens.expires_in {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    let expires_at = now + exp - 60;
                    auth.insert("expires_at".to_string(), expires_at.to_string());
                }
                auth.insert("client_id".to_string(), client_id.clone());
                if let Some(ref cs) = client_secret {
                    auth.insert("client_secret".to_string(), cs.clone());
                }

                let store = FileAuthStore::new_default();
                store
                    .save_profile(info.name, auth_profile, &auth)
                    .map_err(|e| {
                        CommandError::InvalidConfig(format!("Failed to save tokens: {}", e))
                    })?;

                println!("{}", "Authorization successful!".green().bold());
                println!();
                println!("Credentials saved to: {}", store.config_path().dimmed());
                if auth_profile != "default" {
                    println!("{}", format!("Auth profile: {}", auth_profile).dimmed());
                }
                println!();
                println!("{}", "You're all set! Try:".bold());
                let example = match info.name {
                    "google-drive" => "rzn-tools google-drive list-files".to_string(),
                    "google-gmail" => "rzn-tools google-gmail list-messages".to_string(),
                    "google-calendar" => "rzn-tools google-calendar list-events".to_string(),
                    "google-search-console" => {
                        "rzn-tools google-search-console list-sites".to_string()
                    }
                    "microsoft-graph" => {
                        "rzn-tools microsoft-graph list-messages --top 10".to_string()
                    }
                    _ => format!("rzn-tools tools {}", info.name),
                };
                println!("  {}", example.cyan());

                return Ok(());
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("authorization_pending") || err_str.contains("slow_down") {
                    // Still waiting, show progress
                    print!(
                        "\r{} ",
                        format!("Waiting... ({}/{})", attempt + 1, max_attempts).dimmed()
                    );
                    io::stdout().flush()?;
                    continue;
                } else if err_str.contains("access_denied") || err_str.contains("expired") {
                    println!();
                    println!(
                        "{} Authorization was denied or expired.",
                        "Error:".red().bold()
                    );
                    println!(
                        "Run {} to try again.",
                        format!("rzn-tools setup {}", info.name).cyan()
                    );
                    return Ok(());
                } else {
                    // Other error, might still be pending
                    continue;
                }
            }
        }
    }

    println!();
    println!(
        "{} Authorization timed out. Please try again.",
        "Error:".red().bold()
    );

    Ok(())
}

async fn test_connector_auth(cli: &Cli, connector_name: &str) -> Result<()> {
    let store = FileAuthStore::new_default();
    let auth_profile = match cli.auth_profile.as_deref() {
        Some(p) => Some(p.to_string()),
        None => store.resolve_profile_for_provider(connector_name),
    };

    let registry = crate::commands::list::create_registry(auth_profile.as_deref()).await?;
    let provider = registry
        .get_provider(connector_name)
        .ok_or_else(|| CommandError::ConnectorNotFound(connector_name.to_string()))?;

    let mut c = provider.lock().await;

    // Load saved credentials and set them on the connector
    if let Some(profile) = auth_profile.as_deref() {
        if let Some(auth) = store.load_profile(connector_name, profile) {
            c.set_auth_details(auth).await.map_err(|e| {
                CommandError::InvalidConfig(format!("Failed to set credentials: {}", e))
            })?;
        }
    }

    c.test_auth()
        .await
        .map_err(|e| CommandError::InvalidConfig(format!("Authentication test failed: {}", e)))?;

    Ok(())
}

fn show_later_instructions(info: &ConnectorSetupInfo) {
    println!();
    println!("You can configure later with:");
    println!("  {}", format!("rzn-tools setup {}", info.name).cyan());
}

fn read_secret() -> Result<String> {
    // Use rpassword for hidden input
    match rpassword::read_password() {
        Ok(password) => Ok(password.trim().to_string()),
        Err(_) => {
            // Fallback to regular input if rpassword fails (e.g., in non-TTY)
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            Ok(input.trim().to_string())
        }
    }
}
