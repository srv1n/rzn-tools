// src/lib.rs
pub mod auth;
pub mod auth_store;
pub mod capabilities; // Keep for config schema
pub mod connectors;
pub mod cpu_pool;
pub mod display;
pub mod error;
pub mod federated;
pub mod flow_failure;
pub mod ingest;
pub mod logging;
pub mod mcp_server;
pub mod metered;
pub mod oauth;
pub mod oauth_client;
pub mod paths;
pub mod prompts;
pub mod resolver;
pub mod resources;
pub mod system_metadata;
pub mod tools;
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
#[cfg(all(feature = "browser-cookies", target_os = "macos"))]
pub use rookie::safari;
#[cfg(feature = "browser-cookies")]
pub use rookie::{brave, chrome, common::enums::CookieToString, edge, firefox};
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

    /// Returns the MCP capabilities of this connector.
    async fn capabilities(&self) -> ServerCapabilities; // Use MCP's ServerCapabilities

    // --- MCP Request Handlers (One for each relevant MCP request type) ---
    async fn initialize(
        &self,
        request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError>;
    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError>;
    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError>;
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
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError>;
    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError>; // Still a single prompt

    // --- Authentication and Configuration (Keep these) ---

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError>;
    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError>;
    async fn test_auth(&self) -> Result<(), ConnectorError>;
    fn config_schema(&self) -> ConnectorConfigSchema;
}
// ProviderRegistry and ServerInfo remain the same

pub struct ProviderRegistry {
    pub providers: HashMap<String, Arc<tokio::sync::Mutex<Box<dyn Connector>>>>,
    pub aliases: HashMap<String, String>, // alias -> canonical name
}

impl ProviderRegistry {
    pub fn new() -> Self {
        ProviderRegistry {
            providers: HashMap::new(),
            aliases: HashMap::new(),
        }
    }
    pub fn register_provider(&mut self, provider: Box<dyn Connector>) {
        self.providers.insert(
            provider.name().to_string(),
            Arc::new(tokio::sync::Mutex::new(provider)),
        );
    }

    pub fn register_alias(&mut self, alias: &str, canonical_name: &str) {
        self.aliases
            .insert(alias.to_string(), canonical_name.to_string());
    }

    pub fn retain_connectors(&mut self, allowed: &HashSet<String>) {
        self.providers
            .retain(|name, _| allowed.contains(name.as_str()));
        self.aliases
            .retain(|_, canonical_name| self.providers.contains_key(canonical_name));
    }

    pub fn with_usage(self, usage: Arc<UsageManager>) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        registry.aliases = self.aliases;
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
        // First try direct lookup
        if let Some(provider) = self.providers.get(name) {
            return Some(provider);
        }
        // Then try alias lookup
        if let Some(canonical_name) = self.aliases.get(name) {
            return self.providers.get(canonical_name);
        }
        None
    }
    pub fn get_provider_mut(&mut self, _name: &str) -> Option<&mut Box<dyn Connector>> {
        // You usually won't need get_provider_mut with Arc.  Remove it if not needed.
        // self.providers.get_mut(name).map(|arc| Arc::get_mut(arc).expect("Mutable reference to connector requested, but it's shared"))
        None
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
    pub fn get_provider_details(&self) -> Vec<ServerInfo> {
        self.list_providers()
    }

    pub async fn get_provider_capabilities(&self) -> Vec<ServerCapabilities> {
        let mut results = Vec::new();
        for provider in self.providers.values() {
            let c = provider.lock().await;
            results.push(c.capabilities().await);
        }
        results
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

    #[cfg(feature = "hackernews")]
    {
        let connector = connectors::hackernews::HackerNewsConnector::new();
        registry.register_provider(Box::new(connector));
    }

    #[cfg(feature = "wikipedia")]
    {
        if let Ok(connector) =
            connectors::wikipedia::WikipediaConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "youtube")]
    {
        if let Ok(connector) = connectors::youtube::YouTubeConnector::new(None).await {
            registry.register_provider(Box::new(connector));
            registry.register_alias("youtube_transcripts", "youtube");
        }
    }

    #[cfg(feature = "arxiv")]
    {
        if let Ok(connector) =
            connectors::arxiv::ArxivConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "biorxiv")]
    {
        if let Ok(connector) =
            connectors::biorxiv::BiorxivConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "rss")]
    {
        if let Ok(connector) = connectors::rss::RssConnector::new(auth::AuthDetails::new()).await {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "weather")]
    {
        if let Ok(connector) =
            connectors::weather::WeatherConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("wttr", "weather");
        }
    }

    #[cfg(feature = "polymarket")]
    {
        if let Ok(connector) = connectors::polymarket::PolymarketConnector::new().await {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "kalshi")]
    {
        if let Ok(connector) = connectors::kalshi::KalshiConnector::new().await {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "discord")]
    {
        if let Ok(connector) =
            connectors::discord::DiscordConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "google-scholar")]
    {
        if let Ok(connector) =
            connectors::google_scholar::GoogleScholarConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "pubmed")]
    {
        if let Ok(connector) = connectors::pubmed::PubMedConnector::new().await {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "semantic-scholar")]
    {
        if let Ok(connector) =
            connectors::semantic_scholar::SemanticScholarConnector::new(auth::AuthDetails::new())
                .await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("semantic_scholar", "semantic-scholar");
        }
    }

    #[cfg(any(feature = "web", feature = "web-lite"))]
    {
        if let Ok(connector) = connectors::web::WebConnector::new(auth::AuthDetails::new()).await {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "play-store")]
    {
        if let Ok(connector) =
            connectors::play_store::PlayStoreConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("play_store", "play-store");
        }
    }

    #[cfg(feature = "app-store")]
    {
        if let Ok(connector) =
            connectors::app_store::AppStoreConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("appstore", "app-store");
        }
    }

    #[cfg(feature = "app-store-connect")]
    {
        if let Ok(connector) =
            connectors::app_store_connect::AppStoreConnectConnector::new(auth::AuthDetails::new())
                .await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("asc", "app-store-connect");
            registry.register_alias("appstoreconnect", "app-store-connect");
        }
    }

    #[cfg(feature = "apple-search-ads")]
    {
        if let Ok(connector) =
            connectors::apple_search_ads::AppleSearchAdsConnector::new(auth::AuthDetails::new())
                .await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("asa", "apple-search-ads");
            registry.register_alias("apple-searchads", "apple-search-ads");
        }
    }

    #[cfg(feature = "reddit")]
    {
        // Use empty/default auth; downstream can call set_auth later.
        if let Ok(connector) =
            connectors::reddit::RedditConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "linkedin")]
    {
        if let Ok(connector) =
            connectors::linkedin::LinkedInConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "x-api")]
    {
        if let Ok(connector) = connectors::x::XApiConnector::new(auth::AuthDetails::new()).await {
            registry.register_provider(Box::new(connector));
            registry.register_alias("x-api", "x");
            registry.register_alias("twitter-api", "x");
        }
    }

    #[cfg(feature = "x-twitter")]
    {
        if let Ok(connector) =
            connectors::x_browser::XConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("x-cookies", "x-browser");
            registry.register_alias("twitter-cookies", "x-browser");
        }
    }

    #[cfg(feature = "scihub")]
    {
        if let Ok(connector) =
            connectors::scihub::SciHubConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "imap")]
    {
        if let Ok(connector) = connectors::imap::ImapConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "smtp")]
    {
        if let Ok(connector) = connectors::smtp::SmtpConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "caldav")]
    {
        if let Ok(connector) =
            connectors::caldav::CaldavConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    // Productivity & Cloud
    #[cfg(feature = "microsoft-graph")]
    {
        if let Ok(connector) =
            connectors::microsoft::GraphConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "google-drive")]
    {
        if let Ok(connector) =
            connectors::google_drive::DriveConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "google-gmail")]
    {
        if let Ok(connector) =
            connectors::google_gmail::GmailConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "google-calendar")]
    {
        if let Ok(connector) =
            connectors::google_calendar::GoogleCalendarConnector::new(auth::AuthDetails::new())
                .await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "google-people")]
    {
        if let Ok(connector) =
            connectors::google_people::GooglePeopleConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "google-search-console")]
    {
        if let Ok(connector) = connectors::google_search_console::GoogleSearchConsoleConnector::new(
            auth::AuthDetails::new(),
        )
        .await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("gsc", "google-search-console");
        }
    }

    #[cfg(feature = "bing-webmaster-tools")]
    {
        if let Ok(connector) = connectors::bing_webmaster_tools::BingWebmasterToolsConnector::new(
            auth::AuthDetails::new(),
        )
        .await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("bing-webmaster", "bing-webmaster-tools");
            registry.register_alias("bing-search-console", "bing-webmaster-tools");
        }
    }

    #[cfg(feature = "macos-automation")]
    {
        let connector = connectors::macos::MacOsAutomationConnector::new();
        registry.register_provider(Box::new(connector));
    }

    #[cfg(all(target_os = "macos", feature = "macos-spotlight"))]
    {
        let connector = connectors::spotlight::SpotlightConnector::new();
        registry.register_provider(Box::new(connector));
    }

    // Apple Ecosystem connectors (macOS only)
    #[cfg(all(target_os = "macos", feature = "apple-mail"))]
    {
        let connector = connectors::apple_mail::AppleMailConnector::new();
        registry.register_provider(Box::new(connector));
    }

    #[cfg(all(target_os = "macos", feature = "apple-notes"))]
    {
        let connector = connectors::apple_notes::AppleNotesConnector::new();
        registry.register_provider(Box::new(connector));
    }

    #[cfg(all(target_os = "macos", feature = "apple-messages"))]
    {
        let connector = connectors::apple_messages::AppleMessagesConnector::new();
        registry.register_provider(Box::new(connector));
    }

    #[cfg(all(target_os = "macos", feature = "apple-reminders"))]
    {
        let connector = connectors::apple_reminders::AppleRemindersConnector::new();
        registry.register_provider(Box::new(connector));
    }

    #[cfg(all(target_os = "macos", feature = "apple-contacts"))]
    {
        let connector = connectors::apple_contacts::AppleContactsConnector::new();
        registry.register_provider(Box::new(connector));
    }

    // EXPERIMENTAL - NOT READY: HealthKit data store not available on macOS
    // See: rzn_tools_core/src/connectors/apple_health/NOT_READY.md
    // #[cfg(all(target_os = "macos", feature = "apple-health"))]
    // {
    //     let connector = connectors::apple_health::AppleHealthConnector::new();
    //     registry.register_provider(Box::new(connector));
    // }

    #[cfg(feature = "slack")]
    {
        if let Ok(connector) =
            connectors::slack::SlackConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "telegram")]
    {
        if let Ok(connector) =
            connectors::telegram::TelegramConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "whatsapp")]
    {
        if let Ok(connector) =
            connectors::whatsapp::WhatsAppConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "github")]
    {
        if let Ok(connector) =
            connectors::github::GitHubConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "atlassian")]
    {
        if let Ok(connector) =
            connectors::atlassian::AtlassianConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    // LLM provider web search
    #[cfg(feature = "openai-search")]
    {
        if let Ok(connector) =
            connectors::openai_search::OpenAIWebSearchConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "anthropic-search")]
    {
        if let Ok(connector) =
            connectors::anthropic_search::AnthropicWebSearchConnector::new(auth::AuthDetails::new())
                .await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "gemini-search")]
    {
        if let Ok(connector) =
            connectors::gemini_search::GeminiSearchConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "perplexity-search")]
    {
        if let Ok(connector) =
            connectors::perplexity_search::PerplexitySearchConnector::new(auth::AuthDetails::new())
                .await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "xai-search")]
    {
        if let Ok(connector) =
            connectors::xai_search::XaiSearchConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "exa-search")]
    {
        if let Ok(connector) =
            connectors::exa_search::ExaSearchConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("exa-search", "exa");
        }
    }
    #[cfg(feature = "firecrawl-search")]
    {
        if let Ok(connector) =
            connectors::firecrawl_search::FirecrawlSearchConnector::new(auth::AuthDetails::new())
                .await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "serper-search")]
    {
        if let Ok(connector) =
            connectors::serper_search::SerperSearchConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "tavily-search")]
    {
        if let Ok(connector) =
            connectors::tavily_search::TavilySearchConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
            registry.register_alias("tavily", "tavily-search");
        }
    }
    #[cfg(feature = "serpapi-search")]
    {
        if let Ok(connector) =
            connectors::serpapi_search::SerpapiSearchConnector::new(auth::AuthDetails::new()).await
        {
            registry.register_provider(Box::new(connector));
        }
    }
    #[cfg(feature = "parallel-search")]
    {
        if let Ok(connector) =
            connectors::parallel_search::ParallelSearchConnector::new(auth::AuthDetails::new())
                .await
        {
            registry.register_provider(Box::new(connector));
        }
    }

    #[cfg(feature = "localfs")]
    {
        let connector = connectors::localfs::LocalFsConnector::new();
        registry.register_provider(Box::new(connector));
    }

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
