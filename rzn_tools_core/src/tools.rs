use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    auth::AuthDetails,
    auth_store::AuthStore,
    capabilities::ConnectorConfigSchema,
    display::from_normalized::{
        stash_original_structured_content_in_meta,
        try_convert_normalized_structured_content_to_display_v1,
    },
    metered::MeteredConnector,
    usage::UsageManager,
    CallToolRequestParam, CallToolResult, Connector, ConnectorError, ListToolsResult,
    PaginatedRequestParam, Tool,
};

use serde_json::{Map, Value};
use std::borrow::Cow;
use tokio::sync::Mutex;

/// A simple facade that exposes a unified tool surface across all enabled connectors.
/// - Tool names are namespaced as `provider.action` (e.g., `wikipedia.search`).
/// - Only connectors compiled in via Cargo features are included.
pub struct Tools {
    connectors: HashMap<String, Arc<Mutex<Box<dyn Connector>>>>,
    store: Option<Arc<dyn AuthStore>>,
}

impl Tools {
    /// Build Tools containing only feature-enabled connectors.
    pub async fn build_enabled_only() -> Self {
        #[allow(unused_mut)]
        let mut connectors: HashMap<String, Arc<Mutex<Box<dyn Connector>>>> = HashMap::new();

        #[cfg(feature = "hackernews")]
        {
            let c = Box::new(crate::connectors::hackernews::HackerNewsConnector::new())
                as Box<dyn Connector>;
            connectors.insert("hackernews".to_string(), Arc::new(Mutex::new(c)));
        }

        #[cfg(feature = "wikipedia")]
        {
            if let Ok(c) =
                crate::connectors::wikipedia::WikipediaConnector::new(AuthDetails::new()).await
            {
                connectors.insert("wikipedia".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "youtube")]
        {
            if let Ok(c) = crate::connectors::youtube::YouTubeConnector::new(None).await {
                connectors.insert("youtube".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "arxiv")]
        {
            if let Ok(c) = crate::connectors::arxiv::ArxivConnector::new(AuthDetails::new()).await {
                connectors.insert("arxiv".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "weather")]
        {
            if let Ok(c) =
                crate::connectors::weather::WeatherConnector::new(AuthDetails::new()).await
            {
                connectors.insert("weather".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "polymarket")]
        {
            if let Ok(c) = crate::connectors::polymarket::PolymarketConnector::new().await {
                connectors.insert("polymarket".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "kalshi")]
        {
            if let Ok(c) = crate::connectors::kalshi::KalshiConnector::new().await {
                connectors.insert("kalshi".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "pubmed")]
        {
            if let Ok(c) = crate::connectors::pubmed::PubMedConnector::new().await {
                connectors.insert("pubmed".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "semantic-scholar")]
        {
            if let Ok(c) = crate::connectors::semantic_scholar::SemanticScholarConnector::new(
                AuthDetails::new(),
            )
            .await
            {
                connectors.insert(
                    "semantic-scholar".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(any(feature = "web", feature = "web-lite"))]
        {
            if let Ok(c) = crate::connectors::web::WebConnector::new(AuthDetails::new()).await {
                connectors.insert("web".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "play-store")]
        {
            if let Ok(c) =
                crate::connectors::play_store::PlayStoreConnector::new(AuthDetails::new()).await
            {
                connectors.insert("play-store".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "app-store")]
        {
            if let Ok(c) =
                crate::connectors::app_store::AppStoreConnector::new(AuthDetails::new()).await
            {
                connectors.insert("app-store".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "app-store-connect")]
        {
            if let Ok(c) = crate::connectors::app_store_connect::AppStoreConnectConnector::new(
                AuthDetails::new(),
            )
            .await
            {
                connectors.insert(
                    "app-store-connect".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "apple-search-ads")]
        {
            if let Ok(c) = crate::connectors::apple_search_ads::AppleSearchAdsConnector::new(
                AuthDetails::new(),
            )
            .await
            {
                connectors.insert(
                    "apple-search-ads".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "reddit")]
        {
            if let Ok(c) = crate::connectors::reddit::RedditConnector::new(AuthDetails::new()).await
            {
                connectors.insert("reddit".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "x-api")]
        {
            if let Ok(c) = crate::connectors::x::XApiConnector::new(AuthDetails::new()).await {
                connectors.insert("x".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "x-twitter")]
        {
            if let Ok(c) = crate::connectors::x_browser::XConnector::new(AuthDetails::new()).await {
                connectors.insert("x-browser".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "scihub")]
        {
            if let Ok(c) = crate::connectors::scihub::SciHubConnector::new(AuthDetails::new()).await
            {
                connectors.insert("scihub".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "imap")]
        {
            if let Ok(c) = crate::connectors::imap::ImapConnector::new(AuthDetails::new()).await {
                connectors.insert("imap".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "smtp")]
        {
            if let Ok(c) = crate::connectors::smtp::SmtpConnector::new(AuthDetails::new()).await {
                connectors.insert("smtp".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "caldav")]
        {
            if let Ok(c) = crate::connectors::caldav::CaldavConnector::new(AuthDetails::new()).await
            {
                connectors.insert("caldav".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }
        #[cfg(feature = "macos-automation")]
        {
            let c = crate::connectors::macos::MacOsAutomationConnector::new();
            connectors.insert("macos".to_string(), Arc::new(Mutex::new(Box::new(c))));
        }

        #[cfg(all(target_os = "macos", feature = "apple-messages"))]
        {
            let c = crate::connectors::apple_messages::AppleMessagesConnector::new();
            connectors.insert(
                "apple-messages".to_string(),
                Arc::new(Mutex::new(Box::new(c))),
            );
        }

        // LLM provider web search connectors
        #[cfg(feature = "openai-search")]
        {
            if let Ok(c) =
                crate::connectors::openai_search::OpenAIWebSearchConnector::new(AuthDetails::new())
                    .await
            {
                connectors.insert(
                    "openai-search".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "anthropic-search")]
        {
            if let Ok(c) = crate::connectors::anthropic_search::AnthropicWebSearchConnector::new(
                AuthDetails::new(),
            )
            .await
            {
                connectors.insert(
                    "anthropic-search".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "gemini-search")]
        {
            if let Ok(c) =
                crate::connectors::gemini_search::GeminiSearchConnector::new(AuthDetails::new())
                    .await
            {
                connectors.insert(
                    "gemini-search".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "perplexity-search")]
        {
            if let Ok(c) = crate::connectors::perplexity_search::PerplexitySearchConnector::new(
                AuthDetails::new(),
            )
            .await
            {
                connectors.insert(
                    "perplexity-search".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "xai-search")]
        {
            if let Ok(c) =
                crate::connectors::xai_search::XaiSearchConnector::new(AuthDetails::new()).await
            {
                connectors.insert("xai-search".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "exa-search")]
        {
            if let Ok(c) =
                crate::connectors::exa_search::ExaSearchConnector::new(AuthDetails::new()).await
            {
                connectors.insert("exa-search".to_string(), Arc::new(Mutex::new(Box::new(c))));
            }
        }

        #[cfg(feature = "firecrawl-search")]
        {
            if let Ok(c) = crate::connectors::firecrawl_search::FirecrawlSearchConnector::new(
                AuthDetails::new(),
            )
            .await
            {
                connectors.insert(
                    "firecrawl-search".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "serper-search")]
        {
            if let Ok(c) =
                crate::connectors::serper_search::SerperSearchConnector::new(AuthDetails::new())
                    .await
            {
                connectors.insert(
                    "serper-search".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "tavily-search")]
        {
            if let Ok(c) =
                crate::connectors::tavily_search::TavilySearchConnector::new(AuthDetails::new())
                    .await
            {
                connectors.insert(
                    "tavily-search".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        #[cfg(feature = "serpapi-search")]
        {
            if let Ok(c) =
                crate::connectors::serpapi_search::SerpapiSearchConnector::new(AuthDetails::new())
                    .await
            {
                connectors.insert(
                    "serpapi-search".to_string(),
                    Arc::new(Mutex::new(Box::new(c))),
                );
            }
        }

        Tools {
            connectors,
            store: None,
        }
    }

    /// Build Tools with metering enabled for all connectors.
    pub async fn build_enabled_only_with_usage(usage: Arc<UsageManager>) -> Self {
        let mut tools = Tools::build_enabled_only().await;
        tools.wrap_connectors(usage);
        tools
    }

    /// List all tools across connectors, namespaced as "provider.tool".
    pub async fn list(&self) -> Result<ListToolsResult, ConnectorError> {
        let mut all = Vec::new();
        for (provider, conn) in &self.connectors {
            let c = conn.lock().await;
            if let Ok(list) = c
                .list_tools(Some(PaginatedRequestParam { cursor: None }))
                .await
            {
                for t in list.tools {
                    let namespaced = Tool {
                        name: Cow::Owned(format!("{}.{}", provider, t.name)),
                        title: None,
                        description: t.description,
                        input_schema: t.input_schema,
                        output_schema: None,
                        annotations: t.annotations,
                        icons: None,
                    };
                    all.push(namespaced);
                }
            }
        }
        Ok(ListToolsResult {
            tools: all,
            next_cursor: None,
        })
    }

    /// Call a tool by its namespaced name ("provider.tool").
    pub async fn call(&self, name: &str, args: Value) -> Result<CallToolResult, ConnectorError> {
        let (provider, tool) = match name.split_once('.') {
            Some((p, t)) if !p.is_empty() && !t.is_empty() => (p, t),
            _ => {
                return Err(ConnectorError::InvalidParams(
                    "Tool name must be 'provider.tool'".to_string(),
                ))
            }
        };
        let conn = self
            .connectors
            .get(provider)
            .ok_or_else(|| ConnectorError::ToolNotFound)?
            .clone();

        let mut arg_map: Map<String, Value> = match args {
            Value::Object(map) => map,
            _ => Map::new(),
        };

        let requested_display_v1 = arg_map
            .get("output_format")
            .and_then(|v| v.as_str())
            .is_some_and(|v| v == "display_v1");
        if requested_display_v1 {
            arg_map.insert(
                "output_format".to_string(),
                Value::String("normalized_v1".to_string()),
            );
        }
        let req = CallToolRequestParam {
            name: tool.to_string().into(),
            arguments: Some(arg_map),
        };

        let c = conn.lock().await;
        let mut result = c.call_tool(req).await?;
        if requested_display_v1 && !result.is_error.unwrap_or(false) {
            if let Some(structured) = result.structured_content.as_ref() {
                if let Some(converted) =
                    try_convert_normalized_structured_content_to_display_v1(structured)?
                {
                    stash_original_structured_content_in_meta(
                        &mut result.meta,
                        structured,
                        "normalized_v1",
                    );
                    result.structured_content = Some(converted);
                }
            }
        }
        Ok(result)
    }

    /// Set authentication details for a specific provider.
    pub async fn set_auth(
        &self,
        provider: &str,
        details: AuthDetails,
    ) -> Result<(), ConnectorError> {
        let conn = self
            .connectors
            .get(provider)
            .ok_or_else(|| ConnectorError::ToolNotFound)?
            .clone();
        let mut c = conn.lock().await;
        c.set_auth_details(details.clone()).await?;
        if let Some(store) = &self.store {
            let _ = store.save(provider, &details);
        }
        Ok(())
    }

    /// Run the connector's auth test, if supported.
    ///
    /// This is the recommended way to validate credentials end-to-end without invoking a
    /// specific tool. Connectors should implement `test_auth()` with a cheap, read-only
    /// operation (e.g., IMAP NOOP).
    pub async fn test_auth(&self, provider: &str) -> Result<(), ConnectorError> {
        let conn = self
            .connectors
            .get(provider)
            .ok_or_else(|| ConnectorError::ToolNotFound)?
            .clone();
        let c = conn.lock().await;
        c.test_auth().await
    }

    /// Return a connector's config schema to drive UIs.
    pub async fn config_schema(
        &self,
        provider: &str,
    ) -> Result<ConnectorConfigSchema, ConnectorError> {
        let conn = self
            .connectors
            .get(provider)
            .ok_or_else(|| ConnectorError::ToolNotFound)?
            .clone();
        let c = conn.lock().await;
        Ok(c.config_schema())
    }

    /// Return the provider names compiled in this build.
    pub fn list_providers(&self) -> Vec<String> {
        let mut v: Vec<String> = self.connectors.keys().cloned().collect();
        v.sort();
        v
    }

    fn wrap_connectors(&mut self, usage: Arc<UsageManager>) {
        let existing = std::mem::take(&mut self.connectors);
        let mut wrapped = HashMap::new();
        for (name, conn) in existing {
            match Arc::try_unwrap(conn) {
                Ok(mutex) => {
                    let inner = mutex.into_inner();
                    let boxed: Box<dyn Connector> =
                        Box::new(MeteredConnector::new(inner, usage.clone()));
                    wrapped.insert(name, Arc::new(Mutex::new(boxed)));
                }
                Err(conn) => {
                    wrapped.insert(name, conn);
                }
            }
        }
        self.connectors = wrapped;
    }

    /// Describe a namespaced tool (provider.tool).
    pub async fn describe(&self, name: &str) -> Result<Tool, ConnectorError> {
        let (provider, tool) = match name.split_once('.') {
            Some((p, t)) if !p.is_empty() && !t.is_empty() => (p, t),
            _ => {
                return Err(ConnectorError::InvalidParams(
                    "Tool name must be 'provider.tool'".to_string(),
                ))
            }
        };
        let conn = self
            .connectors
            .get(provider)
            .ok_or_else(|| ConnectorError::ToolNotFound)?
            .clone();
        let c = conn.lock().await;
        let list = c
            .list_tools(Some(PaginatedRequestParam { cursor: None }))
            .await?;
        for t in list.tools {
            if t.name == tool {
                return Ok(Tool {
                    name: t.name,
                    title: None,
                    description: t.description,
                    input_schema: t.input_schema,
                    output_schema: None,
                    annotations: t.annotations,
                    icons: None,
                });
            }
        }
        Err(ConnectorError::ToolNotFound)
    }

    /// Convenience for dev shells; desktop apps should prefer AuthStore/with_auth.
    pub async fn auth_from_env(&self) {
        use std::env;
        // Reddit
        if let (Ok(id), Ok(secret)) = (
            env::var("REDDIT_CLIENT_ID"),
            env::var("REDDIT_CLIENT_SECRET"),
        ) {
            let mut auth = AuthDetails::new();
            auth.insert("client_id".into(), id);
            auth.insert("client_secret".into(), secret);
            if let Ok(user) = env::var("REDDIT_USERNAME") {
                auth.insert("username".into(), user);
            }
            if let Ok(pass) = env::var("REDDIT_PASSWORD") {
                auth.insert("password".into(), pass);
            }
            let _ = self.set_auth("reddit", auth).await;
        }
        // X/Twitter (bearer or username/password depending on connector expectations)
        if let Ok(bearer) = env::var("X_BEARER_TOKEN") {
            let mut auth = AuthDetails::new();
            auth.insert("bearer_token".into(), bearer);
            let _ = self.set_auth("x", auth).await;
        }
        // Wikipedia options (language)
        if let Ok(lang) = env::var("WIKIPEDIA_LANG") {
            let mut auth = AuthDetails::new();
            auth.insert("language".into(), lang);
            let _ = self.set_auth("wikipedia", auth).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{ContentItem, NormalizedPageV1, OutputFormat, Partial, Source};
    use async_trait::async_trait;
    use rmcp::model::{
        CallToolRequestParam, InitializeRequestParam, InitializeResult, ListPromptsResult,
        ListResourcesResult, ListToolsResult, PaginatedRequestParam, Prompt, ProtocolVersion,
        ReadResourceRequestParam, ResourceContents, ServerCapabilities, Tool,
    };
    use serde_json::json;
    use std::borrow::Cow;
    use std::sync::Arc;

    struct FakeConnector;

    #[async_trait]
    impl Connector for FakeConnector {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn description(&self) -> &'static str {
            "fake"
        }

        async fn capabilities(&self) -> ServerCapabilities {
            ServerCapabilities::default()
        }

        async fn initialize(
            &self,
            _request: InitializeRequestParam,
        ) -> Result<InitializeResult, ConnectorError> {
            Ok(InitializeResult {
                protocol_version: ProtocolVersion::LATEST,
                capabilities: ServerCapabilities::default(),
                server_info: crate::Implementation {
                    name: "fake".to_string(),
                    title: None,
                    version: "0.0.0".to_string(),
                    icons: None,
                    website_url: None,
                },
                instructions: None,
            })
        }

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
            Ok(Vec::new())
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParam>,
        ) -> Result<ListToolsResult, ConnectorError> {
            Ok(ListToolsResult {
                tools: vec![Tool {
                    name: Cow::Borrowed("search"),
                    title: None,
                    description: None,
                    input_schema: Arc::new(
                        json!({
                            "type":"object",
                            "properties":{
                                "output_format":{
                                    "type":"string",
                                    "enum":["raw","normalized_v1","display_v1"],
                                    "default":"raw"
                                }
                            },
                            "examples":[{"description":"example","input":{"output_format":"normalized_v1"}}],
                            "_meta":{
                                "category":"search",
                                "supports_output_format": true,
                                "supports_cursor": false,
                                "auth_required": false
                            }
                        })
                        .as_object()
                        .expect("schema object")
                        .clone(),
                    ),
                    output_schema: None,
                    annotations: None,
                    icons: None,
                }],
                next_cursor: None,
            })
        }

        async fn call_tool(
            &self,
            request: CallToolRequestParam,
        ) -> Result<CallToolResult, ConnectorError> {
            let args = request.arguments.unwrap_or_default();
            let output_format = crate::ingest::output_format_from_args(&args)?;
            assert_eq!(output_format, OutputFormat::NormalizedV1);

            let item = ContentItem {
                item_ref: "fake:item:1".to_string(),
                kind: "thing".to_string(),
                canonical_url: None,
                title: Some("Hello".to_string()),
                created_at: None,
                source_updated_at: None,
                authors: Vec::new(),
                tags: Vec::new(),
                metadata: Some(json!({"views": 123})),
                blocks: Vec::new(),
                relationships: Vec::new(),
                truncation: None,
            };
            let page = NormalizedPageV1::new(
                vec![item],
                None,
                false,
                Partial::complete(None),
                Source::new("fake", request.name.as_ref()),
            );
            crate::utils::structured_result(&page)
        }

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

    #[tokio::test]
    async fn tools_call_display_v1_stashes_original_normalized() {
        let mut connectors: HashMap<String, Arc<Mutex<Box<dyn Connector>>>> = HashMap::new();
        connectors.insert(
            "fake".to_string(),
            Arc::new(Mutex::new(Box::new(FakeConnector) as Box<dyn Connector>)),
        );
        let tools = Tools {
            connectors,
            store: None,
        };

        let result = tools
            .call("fake.search", json!({"output_format":"display_v1"}))
            .await
            .expect("call");
        let structured = result.structured_content.expect("structured");
        assert_eq!(
            structured.get("type").and_then(|v| v.as_str()),
            Some(crate::display::v1::DISPLAY_PAGE_V1_TYPE)
        );

        let meta = result.meta.expect("meta");
        let original = meta
            .0
            .get(crate::display::from_normalized::META_ORIGINAL_STRUCTURED_CONTENT_KEY)
            .expect("original structured");
        assert_eq!(
            original.get("type").and_then(|v| v.as_str()),
            Some(crate::ingest::NORMALIZED_PAGE_V1_TYPE)
        );
    }
}

/// Builder for Tools with app-managed auth flow and optional store persistence.
pub struct ToolsBuilder {
    auths: HashMap<String, AuthDetails>,
    store: Option<Arc<dyn AuthStore>>,
}

impl ToolsBuilder {
    pub fn new() -> Self {
        Self {
            auths: HashMap::new(),
            store: None,
        }
    }

    pub fn with_auth(mut self, provider: &str, details: AuthDetails) -> Self {
        self.auths.insert(provider.to_string(), details);
        self
    }

    pub fn with_auth_bulk(mut self, map: HashMap<String, AuthDetails>) -> Self {
        self.auths.extend(map);
        self
    }

    pub fn with_auth_store(mut self, store: Arc<dyn AuthStore>) -> Self {
        self.store = Some(store);
        self
    }

    pub async fn build(self) -> Result<Tools, ConnectorError> {
        let mut tools = Tools::build_enabled_only().await;
        tools.store = self.store.clone();

        // Load persisted auths first
        if let Some(store) = &self.store {
            for provider in tools.connectors.keys() {
                if let Some(auth) = store.load(provider) {
                    let _ = tools.set_auth(provider, auth).await; // ignore errors to keep building
                }
            }
        }

        // Overlay with explicitly provided auths
        for (provider, auth) in self.auths.into_iter() {
            let _ = tools.set_auth(&provider, auth).await;
        }

        Ok(tools)
    }
}

impl Default for ToolsBuilder {
    fn default() -> Self {
        Self::new()
    }
}
