// src/lib.rs
pub mod auth;
pub mod auth_store;
pub mod capabilities; // Keep for config schema
pub mod connectors;
pub mod display;
pub mod error;
pub mod federated;
pub mod flow_failure;
pub mod ingest;
pub mod mcp_server;
pub mod metered;
pub mod oauth;
pub mod oauth_client;
pub mod paths;
pub mod resolver;
pub mod system_metadata;
pub mod transport;
pub mod usage;
pub mod usage_context;
pub mod utils;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

// Re-export types from rmcp that users of your library might need
pub use rmcp::model::{
    Annotated, CallToolRequestParam, CallToolResult, Content, Implementation,
    InitializeRequestParam, InitializeResult, IntoContents, ListPromptsResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParam, Prompt, ProtocolVersion, RawContent, RawResource,
    ReadResourceRequestParam, Resource, ResourceContents, ServerCapabilities, TextContent, Tool,
};

use crate::error::ConnectorError;
use crate::metered::MeteredConnector;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
// use crate::capabilities::Capabilities; // Keep for config schema
use crate::auth::AuthDetails;
pub use crate::capabilities::ConnectorConfigSchema; // Export for CLI usage
pub use crate::usage::{
    FileUsageStore, InMemoryUsageStore, PricingCatalog, RunSummary, UsageEvent, UsageManager,
    UsageStore, UsageSummary,
};
pub use crate::usage_context::UsageContext;

/// Specification for URL patterns a connector can handle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct URLPatternSpec {
    /// Regex pattern to match URLs.
    pub pattern: String,
    /// Default tool to invoke when matched.
    pub default_tool: String,
    /// Human-readable description of the pattern.
    pub description: String,
    /// How to extract parameters from the URL.
    pub param_extraction: Vec<URLParamExtraction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct URLParamExtraction {
    /// Capture group index (1-based).
    pub capture_group: usize,
    /// Parameter name to pass to the tool.
    pub param_name: String,
    /// Whether to pass the full URL instead of the capture group.
    pub use_full_url: bool,
}

#[async_trait]
pub trait Connector: Send + Sync {
    /// Returns the unique name of the connector (acting as the MCP server name).
    fn name(&self) -> &'static str;

    /// Returns a description of the connector.
    fn description(&self) -> &'static str;

    /// Human-readable display name for UI.
    fn display_name(&self) -> &'static str {
        self.name()
    }

    /// Emoji or icon identifier for the connector.
    fn icon(&self) -> &'static str {
        "tool"
    }

    /// URL patterns this connector can handle.
    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        Vec::new()
    }

    /// Categories this connector belongs to.
    fn categories(&self) -> Vec<&'static str> {
        Vec::new()
    }

    /// Whether this connector requires authentication.
    fn requires_auth(&self) -> bool {
        false
    }

    /// Returns the canonical provider name for credential lookup.
    ///
    /// This is the key used to look up credentials in the auth store.
    /// Defaults to the connector name. Override for connectors that share
    /// credentials with other systems (e.g., LLM providers).
    ///
    /// # Example
    /// - `openai-search` connector returns `"openai"` to share credentials with OpenAI LLM
    /// - `slack` connector returns `"slack"` (same as name, uses default)
    fn credential_provider(&self) -> &'static str {
        self.name()
    }

    // --- MCP Request Handlers (One for each relevant MCP request type) ---
    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: Vec::new(),
            next_cursor: None,
        })
    }
    async fn read_resource(
        &self,
        _request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError>;
    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError>;
    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: Vec::new(),
            next_cursor: None,
        })
    }
    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::ToolNotFound)
    }

    // --- Authentication and Configuration ---

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(AuthDetails::new())
    }
    async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
        Ok(())
    }
    async fn test_auth(&self) -> Result<(), ConnectorError> {
        Ok(())
    }
    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema::default()
    }
}
pub struct ProviderRegistry {
    pub providers: HashMap<String, Arc<tokio::sync::Mutex<Box<dyn Connector>>>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        ProviderRegistry {
            providers: HashMap::new(),
        }
    }
    pub fn register_provider(&mut self, provider: Box<dyn Connector>) {
        self.providers.insert(
            provider.name().to_string(),
            Arc::new(tokio::sync::Mutex::new(provider)),
        );
    }

    pub fn retain_connectors(&mut self, allowed: &HashSet<String>) {
        self.providers
            .retain(|name, _| allowed.contains(name.as_str()));
    }

    pub fn with_usage(self, usage: Arc<UsageManager>) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        for (name, provider) in self.providers {
            match Arc::try_unwrap(provider) {
                Ok(mutex) => {
                    let inner = mutex.into_inner();
                    let wrapped: Box<dyn Connector> =
                        Box::new(MeteredConnector::new(inner, usage.clone()));
                    registry
                        .providers
                        .insert(name, Arc::new(tokio::sync::Mutex::new(wrapped)));
                }
                Err(provider) => {
                    registry.providers.insert(name, provider);
                }
            }
        }
        registry
    }

    pub fn get_provider(&self, name: &str) -> Option<&Arc<tokio::sync::Mutex<Box<dyn Connector>>>> {
        self.providers.get(name)
    }
    pub fn list_providers(&self) -> Vec<ServerInfo> {
        self.providers
            .iter()
            .map(|(name, connector)| {
                if let Ok(c) = connector.try_lock() {
                    ServerInfo {
                        name: name.clone(),
                        description: c.description().to_string(),
                    }
                } else {
                    ServerInfo {
                        name: name.clone(),
                        description: String::new(),
                    }
                }
            })
            .collect()
    }
    pub async fn get_provider_tools(&self) -> Vec<Tool> {
        let mut all_tools = Vec::new();
        for provider in self.providers.values() {
            let c = provider.lock().await;
            if let Ok(response) = c.list_tools(None).await {
                all_tools.extend(response.tools);
            }
        }
        all_tools
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a registry that registers only connectors enabled via Cargo features.
/// This is useful for downstream apps to depend on a minimal feature set and get
/// a ready-to-use registry without manually wiring each connector.
pub async fn build_registry_enabled_only() -> ProviderRegistry {
    #[allow(unused_mut)]
    let mut registry = ProviderRegistry::new();

    #[allow(unused_macros)]
    macro_rules! register_async {
        ($connector:expr) => {
            if let Ok(connector) = $connector {
                registry.register_provider(Box::new(connector));
            }
        };
    }

    #[allow(unused_macros)]
    macro_rules! register_sync {
        ($connector:expr) => {{
            registry.register_provider(Box::new($connector));
        }};
    }

    #[cfg(feature = "hackernews")]
    register_sync!(connectors::hackernews::HackerNewsConnector::new());

    #[cfg(feature = "wikipedia")]
    register_async!(connectors::wikipedia::WikipediaConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "youtube")]
    register_async!(connectors::youtube::YouTubeConnector::new(None).await);

    #[cfg(feature = "arxiv")]
    register_async!(connectors::arxiv::ArxivConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "biorxiv")]
    register_async!(connectors::biorxiv::BiorxivConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "rss")]
    register_async!(connectors::rss::RssConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "weather")]
    register_async!(connectors::weather::WeatherConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "polymarket")]
    register_async!(connectors::polymarket::PolymarketConnector::new().await);

    #[cfg(feature = "kalshi")]
    register_async!(connectors::kalshi::KalshiConnector::new().await);

    #[cfg(feature = "discord")]
    register_async!(connectors::discord::DiscordConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "google-scholar")]
    register_async!(
        connectors::google_scholar::GoogleScholarConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "pubmed")]
    register_async!(connectors::pubmed::PubMedConnector::new().await);

    #[cfg(feature = "semantic-scholar")]
    register_async!(
        connectors::semantic_scholar::SemanticScholarConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "web")]
    register_async!(connectors::web::WebConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "play-store")]
    register_async!(
        connectors::play_store::PlayStoreConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "app-store")]
    register_async!(connectors::app_store::AppStoreConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "app-store-connect")]
    register_async!(
        connectors::app_store_connect::AppStoreConnectConnector::new(auth::AuthDetails::new())
            .await
    );

    #[cfg(feature = "apple-search-ads")]
    register_async!(
        connectors::apple_search_ads::AppleSearchAdsConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "reddit")]
    register_async!(connectors::reddit::RedditConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "linkedin")]
    register_async!(connectors::linkedin::LinkedInConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "x-api")]
    register_async!(connectors::x::XApiConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "x-twitter")]
    register_async!(connectors::x_browser::XConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "scihub")]
    register_async!(connectors::scihub::SciHubConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "imap")]
    register_async!(connectors::imap::ImapConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "smtp")]
    register_async!(connectors::smtp::SmtpConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "caldav")]
    register_async!(connectors::caldav::CaldavConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "microsoft-graph")]
    register_async!(connectors::microsoft::GraphConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "google-drive")]
    register_async!(connectors::google_drive::DriveConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "google-gmail")]
    register_async!(connectors::google_gmail::GmailConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "google-calendar")]
    register_async!(
        connectors::google_calendar::GoogleCalendarConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "google-people")]
    register_async!(
        connectors::google_people::GooglePeopleConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "google-search-console")]
    register_async!(
        connectors::google_search_console::GoogleSearchConsoleConnector::new(
            auth::AuthDetails::new(),
        )
        .await
    );

    #[cfg(feature = "bing-webmaster-tools")]
    register_async!(
        connectors::bing_webmaster_tools::BingWebmasterToolsConnector::new(auth::AuthDetails::new())
            .await
    );

    #[cfg(feature = "macos-automation")]
    register_sync!(connectors::macos::MacOsAutomationConnector::new());

    #[cfg(all(target_os = "macos", feature = "macos-spotlight"))]
    register_sync!(connectors::spotlight::SpotlightConnector::new());

    #[cfg(all(target_os = "macos", feature = "apple-mail"))]
    register_sync!(connectors::apple_mail::AppleMailConnector::new());

    #[cfg(all(target_os = "macos", feature = "apple-notes"))]
    register_sync!(connectors::apple_notes::AppleNotesConnector::new());

    #[cfg(all(target_os = "macos", feature = "apple-messages"))]
    register_sync!(connectors::apple_messages::AppleMessagesConnector::new());

    #[cfg(all(target_os = "macos", feature = "apple-reminders"))]
    register_sync!(connectors::apple_reminders::AppleRemindersConnector::new());

    #[cfg(all(target_os = "macos", feature = "apple-contacts"))]
    register_sync!(connectors::apple_contacts::AppleContactsConnector::new());

    #[cfg(feature = "slack")]
    register_async!(connectors::slack::SlackConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "telegram")]
    register_async!(connectors::telegram::TelegramConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "whatsapp")]
    register_async!(connectors::whatsapp::WhatsAppConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "github")]
    register_async!(connectors::github::GitHubConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "atlassian")]
    register_async!(connectors::atlassian::AtlassianConnector::new(auth::AuthDetails::new()).await);

    #[cfg(feature = "openai-search")]
    register_async!(
        connectors::openai_search::OpenAIWebSearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "anthropic-search")]
    register_async!(
        connectors::anthropic_search::AnthropicWebSearchConnector::new(auth::AuthDetails::new())
            .await
    );

    #[cfg(feature = "gemini-search")]
    register_async!(
        connectors::gemini_search::GeminiSearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "perplexity-search")]
    register_async!(
        connectors::perplexity_search::PerplexitySearchConnector::new(auth::AuthDetails::new())
            .await
    );

    #[cfg(feature = "xai-search")]
    register_async!(
        connectors::xai_search::XaiSearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "exa-search")]
    register_async!(
        connectors::exa_search::ExaSearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "firecrawl-search")]
    register_async!(
        connectors::firecrawl_search::FirecrawlSearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "serper-search")]
    register_async!(
        connectors::serper_search::SerperSearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "tavily-search")]
    register_async!(
        connectors::tavily_search::TavilySearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "serpapi-search")]
    register_async!(
        connectors::serpapi_search::SerpapiSearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "parallel-search")]
    register_async!(
        connectors::parallel_search::ParallelSearchConnector::new(auth::AuthDetails::new()).await
    );

    #[cfg(feature = "localfs")]
    register_sync!(connectors::localfs::LocalFsConnector::new());

    registry
}

/// Build a registry and wrap connectors with usage metering.
pub async fn build_registry_enabled_only_with_usage(usage: Arc<UsageManager>) -> ProviderRegistry {
    let registry = build_registry_enabled_only().await;
    registry.with_usage(usage)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub description: String,
}
