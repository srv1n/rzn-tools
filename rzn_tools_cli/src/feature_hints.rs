#[derive(Debug, Clone, Copy)]
pub struct ConnectorFeatureHint {
    pub canonical: &'static str,
    pub cargo_feature: &'static str,
    pub enabled: bool,
    pub aliases: &'static [&'static str],
}

fn normalize_connector_name(name: &str) -> String {
    name.trim().to_lowercase().replace('_', "-")
}

pub fn hint_for_connector(name: &str) -> Option<ConnectorFeatureHint> {
    let normalized = normalize_connector_name(name);

    // Note: This list is intentionally lightweight metadata and should stay in-sync
    // with rzn_tools_cli/Cargo.toml feature names and CLI command names.
    const HINTS: &[ConnectorFeatureHint] = &[
        ConnectorFeatureHint {
            canonical: "arxiv",
            cargo_feature: "arxiv",
            enabled: cfg!(feature = "arxiv"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "atlassian",
            cargo_feature: "atlassian",
            enabled: cfg!(feature = "atlassian"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "biorxiv",
            cargo_feature: "biorxiv",
            enabled: cfg!(feature = "biorxiv"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "discord",
            cargo_feature: "discord",
            enabled: cfg!(feature = "discord"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "github",
            cargo_feature: "github",
            enabled: cfg!(feature = "github"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "google-scholar",
            cargo_feature: "google-scholar",
            enabled: cfg!(feature = "google-scholar"),
            aliases: &["google_scholar"],
        },
        ConnectorFeatureHint {
            canonical: "google-search-console",
            cargo_feature: "google-search-console",
            enabled: cfg!(feature = "google-search-console"),
            aliases: &["gsc", "search-console", "google_search_console"],
        },
        ConnectorFeatureHint {
            canonical: "hackernews",
            cargo_feature: "hackernews",
            enabled: cfg!(feature = "hackernews"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "bing-webmaster-tools",
            cargo_feature: "bing-webmaster-tools",
            enabled: cfg!(feature = "bing-webmaster-tools"),
            aliases: &["bing-webmaster", "bing-search-console", "bing-webmasters"],
        },
        ConnectorFeatureHint {
            canonical: "imap",
            cargo_feature: "imap",
            enabled: cfg!(feature = "imap"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "smtp",
            cargo_feature: "smtp",
            enabled: cfg!(feature = "smtp"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "caldav",
            cargo_feature: "caldav",
            enabled: cfg!(feature = "caldav"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "localfs",
            cargo_feature: "localfs",
            enabled: cfg!(feature = "localfs"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "pubmed",
            cargo_feature: "pubmed",
            enabled: cfg!(feature = "pubmed"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "polymarket",
            cargo_feature: "polymarket",
            enabled: cfg!(feature = "polymarket"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "kalshi",
            cargo_feature: "kalshi",
            enabled: cfg!(feature = "kalshi"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "reddit",
            cargo_feature: "reddit",
            enabled: cfg!(feature = "reddit"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "play-store",
            cargo_feature: "play-store",
            enabled: cfg!(feature = "play-store"),
            aliases: &["playstore", "google-play", "google_play"],
        },
        ConnectorFeatureHint {
            canonical: "app-store",
            cargo_feature: "app-store",
            enabled: cfg!(feature = "app-store"),
            aliases: &["appstore", "itunes", "ios-app-store"],
        },
        ConnectorFeatureHint {
            canonical: "app-store-connect",
            cargo_feature: "app-store-connect",
            enabled: cfg!(feature = "app-store-connect"),
            aliases: &["asc", "appstoreconnect", "app_store_connect"],
        },
        ConnectorFeatureHint {
            canonical: "apple-search-ads",
            cargo_feature: "apple-search-ads",
            enabled: cfg!(feature = "apple-search-ads"),
            aliases: &["asa", "apple-searchads", "search-ads"],
        },
        ConnectorFeatureHint {
            canonical: "rss",
            cargo_feature: "rss",
            enabled: cfg!(feature = "rss"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "weather",
            cargo_feature: "weather",
            enabled: cfg!(feature = "weather"),
            aliases: &["wttr", "wttr-in"],
        },
        ConnectorFeatureHint {
            canonical: "scihub",
            cargo_feature: "scihub",
            enabled: cfg!(feature = "scihub"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "semantic-scholar",
            cargo_feature: "semantic-scholar",
            enabled: cfg!(feature = "semantic-scholar"),
            aliases: &["semantic_scholar", "scholar"],
        },
        ConnectorFeatureHint {
            canonical: "slack",
            cargo_feature: "slack",
            enabled: cfg!(feature = "slack"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "web",
            cargo_feature: "web",
            enabled: cfg!(feature = "web") || cfg!(feature = "web-lite"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "wikipedia",
            cargo_feature: "wikipedia",
            enabled: cfg!(feature = "wikipedia"),
            aliases: &["wiki"],
        },
        ConnectorFeatureHint {
            canonical: "x",
            cargo_feature: "x-twitter",
            enabled: cfg!(feature = "x-twitter"),
            aliases: &["twitter", "x-twitter"],
        },
        ConnectorFeatureHint {
            canonical: "youtube",
            cargo_feature: "youtube",
            enabled: cfg!(feature = "youtube"),
            aliases: &[],
        },
        // Search provider connectors (command names don't always match feature names)
        ConnectorFeatureHint {
            canonical: "openai-search",
            cargo_feature: "openai-search",
            enabled: cfg!(feature = "openai-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "anthropic-search",
            cargo_feature: "anthropic-search",
            enabled: cfg!(feature = "anthropic-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "gemini-search",
            cargo_feature: "gemini-search",
            enabled: cfg!(feature = "gemini-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "perplexity-search",
            cargo_feature: "perplexity-search",
            enabled: cfg!(feature = "perplexity-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "xai-search",
            cargo_feature: "xai-search",
            enabled: cfg!(feature = "xai-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "exa",
            cargo_feature: "exa-search",
            enabled: cfg!(feature = "exa-search"),
            aliases: &["exa-search"],
        },
        ConnectorFeatureHint {
            canonical: "tavily-search",
            cargo_feature: "tavily-search",
            enabled: cfg!(feature = "tavily-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "serper-search",
            cargo_feature: "serper-search",
            enabled: cfg!(feature = "serper-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "serpapi-search",
            cargo_feature: "serpapi-search",
            enabled: cfg!(feature = "serpapi-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "firecrawl-search",
            cargo_feature: "firecrawl-search",
            enabled: cfg!(feature = "firecrawl-search"),
            aliases: &[],
        },
        ConnectorFeatureHint {
            canonical: "parallel-search",
            cargo_feature: "parallel-search",
            enabled: cfg!(feature = "parallel-search"),
            aliases: &[],
        },
    ];

    HINTS
        .iter()
        .find(|h| {
            normalize_connector_name(h.canonical) == normalized
                || h.aliases
                    .iter()
                    .any(|alias| normalize_connector_name(alias) == normalized)
        })
        .copied()
}
