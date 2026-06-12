use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::ingest::{
    self, Author, ContentBlock, ContentItem, NormalizedItemV1, OutputFormat, Partial, Source,
};
use crate::utils::{
    get_cookies, get_domain, get_user_agent, match_browser, strip_multiple_newlines, Browser,
};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use htmd::HtmlToMarkdown;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, COOKIE, USER_AGENT};
use rmcp::model::*;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";

fn browser_identifier(browser: &Browser) -> &'static str {
    match browser {
        Browser::Firefox => "firefox",
        Browser::Chrome => "chrome",
        Browser::Edge => "edge",
        Browser::Safari => "safari",
        Browser::Brave => "brave",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlParameter {
    pub name: String,
    pub description: String,
    pub is_path: bool,
    pub example: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorConfig {
    pub name: String,
    pub selector: String,
    pub description: Option<String>,
    pub attribute: Option<String>,
    pub fallback_selector: Option<String>,
    pub post_processing: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    pub name: String,
    pub description: String,
    pub url_template: String,
    pub url_parameters: Vec<UrlParameter>,
    pub item_selector: Option<String>,
    pub selectors: Vec<SelectorConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WebContent {
    pub url: String,
    pub title: Option<String>,
    pub content: String,
    pub metadata: WebMetadata,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct WebMetadata {
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub author: Option<String>,
    pub published_date: Option<String>,
}

fn web_item_ref(url: &str) -> String {
    let encoded = URL_SAFE_NO_PAD.encode(url.as_bytes());
    format!("web:page:{}", encoded)
}

fn web_item_from_content(content: &WebContent) -> ContentItem {
    let item_ref = web_item_ref(&content.url);
    let author = content.metadata.author.as_ref().map(|name| Author {
        name: name.to_string(),
        id: None,
    });
    let block = ContentBlock {
        block_ref: format!("{}:content", item_ref),
        block_kind: "content".to_string(),
        text: content.content.clone(),
        author,
        created_at: content.metadata.published_date.clone(),
        reply_to: None,
        position: None,
        score: None,
        attachments: Vec::new(),
        metadata: None,
    };

    ContentItem {
        item_ref,
        kind: "page".to_string(),
        canonical_url: Some(content.url.clone()),
        title: content.title.clone(),
        created_at: content.metadata.published_date.clone(),
        source_updated_at: None,
        authors: content
            .metadata
            .author
            .as_ref()
            .map(|name| {
                vec![Author {
                    name: name.to_string(),
                    id: None,
                }]
            })
            .unwrap_or_default(),
        tags: content.metadata.keywords.clone(),
        metadata: Some(json!({
            "description": content.metadata.description,
            "keywords": content.metadata.keywords,
            "author": content.metadata.author,
            "published_date": content.metadata.published_date
        })),
        blocks: vec![block],
        relationships: Vec::new(),
        truncation: None,
    }
}

#[derive(Clone)]
pub struct WebConnector {
    client: reqwest::Client,
    pub headers: HeaderMap,
    pub browser: Browser,
    cookie_cache: Arc<Mutex<HashMap<String, String>>>,
}

impl WebConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let browser = match_browser(
            auth.get("browser")
                .unwrap_or(&"firefox".to_string())
                .to_string(),
        )
        .await?;

        let client = reqwest::Client::builder()
            .http1_only()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .pool_max_idle_per_host(2)
            .tcp_keepalive(Some(Duration::from_secs(30)))
            .user_agent(DEFAULT_USER_AGENT)
            .build()
            .map_err(|e| ConnectorError::Other(format!("failed to build http client: {}", e)))?;

        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));

        let mut connector = WebConnector {
            browser,
            client,
            headers,
            cookie_cache: Arc::new(Mutex::new(HashMap::new())),
        };

        connector.set_auth_details(auth).await?;
        Ok(connector)
    }

    async fn scrape_url(
        &self,
        url: &str,
        browser: &Browser,
        cookies: Option<&str>,
    ) -> Result<WebContent, ConnectorError> {
        let user_agent = self
            .headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(|ua| ua.to_string())
            .unwrap_or_else(|| get_user_agent(browser.clone()));

        let mut request = self.client.get(url);
        request = request.header(
            USER_AGENT,
            HeaderValue::from_str(&user_agent).map_err(|e| ConnectorError::Other(e.to_string()))?,
        );

        if let Some(cookie_header) = cookies {
            if !cookie_header.is_empty() {
                request = request.header(
                    COOKIE,
                    HeaderValue::from_str(cookie_header)
                        .map_err(|e| ConnectorError::Other(e.to_string()))?,
                );
            }
        }

        let t0 = std::time::Instant::now();
        let resp = request
            .send()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        let t1 = std::time::Instant::now();
        let response = resp
            .text()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        let t2 = std::time::Instant::now();

        debug!(
            target: "connector.web",
            url = %url,
            connect_send_ms = %((t1 - t0).as_millis()),
            read_body_ms = %((t2 - t1).as_millis()),
            total_ms = %((t2 - t0).as_millis()),
            "scraped url"
        );

        let t3 = std::time::Instant::now();
        let content = strip_multiple_newlines(&response);
        let t4 = std::time::Instant::now();

        let html = Html::parse_document(&content);
        let t5 = std::time::Instant::now();
        let main_html = find_main_content(&html);
        let t6 = std::time::Instant::now();
        let content = html_to_markdown(&main_html);
        let t7 = std::time::Instant::now();
        let metadata = self.extract_metadata(&html)?;
        let t8 = std::time::Instant::now();
        let title = html
            .select(&Selector::parse("title").map_err(|e| {
                ConnectorError::Other(format!("Failed to parse title selector: {}", e))
            })?)
            .next()
            .map(|el| el.inner_html());

        debug!(
            target: "connector.web",
            url = %url,
            trim_ms = %((t4 - t3).as_millis()),
            parse_ms = %((t5 - t4).as_millis()),
            find_ms = %((t6 - t5).as_millis()),
            md_ms = %((t7 - t6).as_millis()),
            meta_ms = %((t8 - t7).as_millis()),
            body_bytes = %response.len(),
            content_chars = %content.len(),
            "processed html to markdown"
        );

        Ok(WebContent {
            url: url.to_string(),
            title,
            content,
            metadata,
        })
    }

    async fn resolve_browser_override(
        &self,
        browser_name: Option<&str>,
    ) -> Result<Browser, ConnectorError> {
        if let Some(name) = browser_name {
            match_browser(name.to_string()).await
        } else {
            Ok(self.browser.clone())
        }
    }

    async fn cookies_for_request(
        &self,
        browser: &Browser,
        domain: &str,
        use_cookies: bool,
    ) -> Result<Option<String>, ConnectorError> {
        if !use_cookies {
            return Ok(None);
        }

        if let Some(explicit_cookie) = self
            .headers
            .get(COOKIE)
            .and_then(|value| value.to_str().ok())
            .map(|s| s.to_string())
        {
            return Ok(Some(explicit_cookie));
        }

        let cache_key = format!("{}|{}", browser_identifier(browser), domain);
        if let Some(cached) = {
            let cache = self.cookie_cache.lock().await;
            cache.get(&cache_key).cloned()
        } {
            return Ok(Some(cached));
        }

        let t0 = std::time::Instant::now();
        // Guard rookie cookie extraction with a short timeout; fall back to no cookies
        let cookies_res = tokio::time::timeout(
            Duration::from_millis(500),
            get_cookies(browser.clone(), domain.to_string()),
        )
        .await;

        let cookies = match cookies_res {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => {
                debug!(
                    target: "connector.web",
                    domain = %domain,
                    error = %e,
                    "cookie extraction failed; continuing without cookies"
                );
                return Ok(None);
            }
            Err(_) => {
                debug!(
                    target: "connector.web",
                    domain = %domain,
                    "cookie extraction timed out; continuing without cookies"
                );
                return Ok(None);
            }
        };
        let t1 = std::time::Instant::now();

        debug!(
            target: "connector.web",
            domain = %domain,
            ms = %((t1 - t0).as_millis()),
            "retrieved cookies from browser store"
        );

        {
            let mut cache = self.cookie_cache.lock().await;
            cache.insert(cache_key, cookies.clone());
        }

        Ok(Some(cookies))
    }

    fn extract_metadata(&self, document: &scraper::Html) -> Result<WebMetadata, ConnectorError> {
        let mut metadata = WebMetadata::default();

        // Extract meta description
        if let Ok(selector) = Selector::parse("meta[name='description']") {
            if let Some(desc) = document
                .select(&selector)
                .next()
                .and_then(|el| el.value().attr("content"))
            {
                metadata.description = Some(desc.to_string());
            }
        }

        // Extract meta keywords
        if let Ok(selector) = Selector::parse("meta[name='keywords']") {
            if let Some(keywords) = document
                .select(&selector)
                .next()
                .and_then(|el| el.value().attr("content"))
            {
                metadata.keywords = keywords.split(',').map(|s| s.trim().to_string()).collect();
            }
        }

        // Extract meta author
        if let Ok(selector) = Selector::parse("meta[name='author']") {
            if let Some(author) = document
                .select(&selector)
                .next()
                .and_then(|el| el.value().attr("content"))
            {
                metadata.author = Some(author.to_string());
            }
        }

        // Extract published date
        if let Ok(selector) = Selector::parse("meta[property='article:published_time']") {
            if let Some(date) = document
                .select(&selector)
                .next()
                .and_then(|el| el.value().attr("content"))
            {
                metadata.published_date = Some(date.to_string());
            }
        }

        Ok(metadata)
    }

    fn process_url_template(
        &self,
        template: &str,
        url_parameters: &[UrlParameter],
        parameters: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<String, ConnectorError> {
        // println!("Processing URL template: {}", template);
        // println!("URL parameters: {:#?}", url_parameters);
        // println!("Parameters: {:#?}", parameters);
        let mut result = template.to_string();
        let mut required_params = url_parameters
            .iter()
            .filter(|p| p.is_path)
            .map(|p| p.name.clone())
            .collect::<Vec<String>>();

        // Keep track of parameters that have been used in the template
        let mut used_params = Vec::new();

        // Validate required parameters
        for param in &required_params {
            if !parameters.contains_key(param) {
                return Err(ConnectorError::InvalidParams(format!(
                    "Missing required path parameter: {}",
                    param
                )));
            }
        }

        // Process path parameters (format: {param_name})
        for (key, value) in parameters {
            let placeholder = format!("{{{}}}", key);
            if result.contains(&placeholder) {
                // Remove from required params list once processed
                if let Some(pos) = required_params.iter().position(|p| p == key) {
                    required_params.remove(pos);
                }

                // Mark this parameter as used in the template
                used_params.push(key.clone());

                if let Some(value_str) = value.as_str() {
                    result = result.replace(&placeholder, value_str);
                } else {
                    result = result.replace(&placeholder, &value.to_string());
                }
            }
        }

        // Check if all required parameters were processed
        if !required_params.is_empty() {
            return Err(ConnectorError::InvalidParams(format!(
                "Missing required path parameters: {}",
                required_params.join(", ")
            )));
        }

        // Process query parameters (parameters that aren't path parameters AND weren't used in the template)
        let query_params: Vec<(String, String)> = parameters
            .iter()
            .filter(|(key, _)| {
                !url_parameters.iter().any(|p| p.is_path && &p.name == *key)
                    && !used_params.contains(key)
            })
            .map(|(key, value)| {
                let value_str = if let Some(s) = value.as_str() {
                    s.to_string()
                } else {
                    value.to_string()
                };
                (key.clone(), value_str)
            })
            .collect();

        // Add query parameters if any exist
        if !query_params.is_empty() {
            if result.contains('?') {
                result.push('&');
            } else {
                result.push('?');
            }

            let query_string = query_params
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}={}",
                        urlencoding::encode(key),
                        urlencoding::encode(value)
                    )
                })
                .collect::<Vec<String>>()
                .join("&");

            result.push_str(&query_string);
        }

        Ok(result)
    }
}

#[async_trait]
impl Connector for WebConnector {
    fn name(&self) -> &'static str {
        "web"
    }

    fn description(&self) -> &'static str {
        "A connector for scraping web content with optional cookie support"
    }

    fn display_name(&self) -> &'static str {
        "Web"
    }

    fn icon(&self) -> &'static str {
        "web"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["web", "scrape"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    async fn capabilities(&self) -> ServerCapabilities {
        // Define the capabilities according to what your connector supports.
        ServerCapabilities {
            tools: None,
            ..Default::default() // Use default for other capabilities
        }
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.headers.clear();

        let user_agent = details
            .get("user_agent")
            .cloned()
            .unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());

        let ua_header =
            HeaderValue::from_str(&user_agent).map_err(|e| ConnectorError::Other(e.to_string()))?;
        self.headers.insert(USER_AGENT, ua_header);

        if let Some(browser_name) = details.get("browser") {
            let selected_browser = match_browser(browser_name.to_string())
                .await
                .map_err(|e| ConnectorError::Other(e.to_string()))?;
            self.browser = selected_browser;
        }

        if let Some(cookie_value) = details.get("cookie").or_else(|| details.get("cookies")) {
            let cookie_header = HeaderValue::from_str(cookie_value)
                .map_err(|e| ConnectorError::Other(e.to_string()))?;
            self.headers.insert(COOKIE, cookie_header);
        }

        self.cookie_cache.lock().await.clear();
        Ok(())
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(AuthDetails::new())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Test scraping a simple website without requiring cookies
        self.scrape_url("https://example.com", &self.browser, None)
            .await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        let browser_options: Vec<String> = ["firefox", "chrome", "edge", "safari", "brave"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "browser".to_string(),
                    label: "Browser Profile".to_string(),
                    field_type: FieldType::Select {
                        options: browser_options.clone(),
                    },
                    required: false,
                    description: Some(
                        "Which browser profile to use when extracting cookies via Rookie.".into(),
                    ),
                    options: Some(browser_options),
                },
                Field {
                    name: "user_agent".to_string(),
                    label: "User Agent".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Override the default user agent string sent with requests.".into(),
                    ),
                    options: None,
                },
                Field {
                    name: "cookie".to_string(),
                    label: "Cookie Header".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Optional raw Cookie header value to include when cookies are enabled."
                            .into(),
                    ),
                    options: None,
                },
            ],
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        Ok(ListToolsResult {
            tools: vec![
                Tool {
                    name: Cow::Borrowed("scrape_url"),
                    title: None,
                    description: Some(Cow::Borrowed(
                        "Extract readable text + basic metadata from a URL. Use when you want \
the main page content (not structured scraping). Example: url=\"https://example.com\".",
                    )),
                    annotations: None,
                    input_schema: Arc::new(json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "The URL to scrape"
                            },
                            "use_cookies": {
                                "type": "boolean",
                                "description": "Whether to use browser cookies (defaults to false to avoid OS Keychain prompts and slowdowns)",
                                "default": false
                            },
                        "browser": {
                            "type": "string",
                            "description": "Override the browser profile used to resolve cookies and user agent",
                            "enum": ["firefox", "chrome", "edge", "safari", "brave"],
                            "default": "firefox"
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "normalized_v1", "display_v1"],
                            "default": "raw",
                            "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                        }
                    },
                        "required": ["url"],
                        "examples": [
                            { "description": "Scrape a page", "input": { "url": "https://example.com" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["web", "scrape"],
                            "auth_required": false,
                            "supports_output_format": true,
                            "supports_cursor": false
                        }
                    }).as_object().expect("Schema object").clone()),
                    output_schema: None,
                    icons: None,
                },
                Tool {
                    name: Cow::Borrowed("get"),
                    title: None,
                    description: Some(Cow::Borrowed(
                        "Fetch a URL and return readable content (alias of scrape_url).",
                    )),
                    annotations: None,
                    input_schema: Arc::new(json!({
                        "type": "object",
                        "properties": {
                            "url": {
                                "type": "string",
                                "description": "The URL to scrape"
                            },
                            "use_cookies": {
                                "type": "boolean",
                                "description": "Whether to use browser cookies (defaults to false to avoid OS Keychain prompts and slowdowns)",
                                "default": false
                            },
                            "browser": {
                                "type": "string",
                                "description": "Override the browser profile used to resolve cookies and user agent",
                                "enum": ["firefox", "chrome", "edge", "safari", "brave"],
                                "default": "firefox"
                            },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1", "display_v1"],
                                "default": "raw",
                                "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output."
                            }
                        },
                        "required": ["url"],
                        "examples": [
                            { "description": "Fetch a page", "input": { "url": "https://example.com" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["web", "scrape"],
                            "auth_required": false,
                            "supports_output_format": true,
                            "supports_cursor": false
                        }
                    }).as_object().expect("Schema object").clone()),
                    output_schema: None,
                    icons: None,
                },
                Tool {
                    name: Cow::Borrowed("scrape_with_config"),
                    title: None,
                    description: Some(Cow::Borrowed(
                        "Scrape with explicit CSS selectors for structured extraction. Use when \
you need specific fields (e.g., title/price) and scrape_url is too noisy.",
                    )),
                    annotations: None,
                    input_schema: Arc::new(json!({
                        "type": "object",
                        "properties": {
                            "tool": {
                                "type": "object",
                                "description": "Declarative scrape config",
                                "properties": {
                                    "name": {"type": "string"},
                                    "description": {"type": "string"},
                                    "url_template": {"type": "string"},
                                    "url_parameters": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "name": {"type": "string"},
                                                "description": {"type": "string"},
                                                "is_path": {"type": "boolean"},
                                                "example": {"type": "string"}
                                            },
                                            "required": ["name", "is_path"]
                                        }
                                    },
                                    "item_selector": {"type": ["string", "null"]},
                                    "selectors": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "name": {"type": "string"},
                                                "selector": {"type": "string"},
                                                "description": {"type": ["string", "null"]},
                                                "attribute": {"type": ["string", "null"]},
                                                "fallback_selector": {"type": ["string", "null"]},
                                                "post_processing": {
                                                    "type": ["array", "null"],
                                                    "items": {"type": "string"}
                                                }
                                            },
                                            "required": ["name", "selector"]
                                        }
                                    }
                                },
                                "required": ["url_template", "selectors", "url_parameters"]
                            },
                            "parameters": {
                                "type": "object",
                                "description": "Values for template parameters",
                                "additionalProperties": true
                            },
                            "use_cookies": {
                                "type": "boolean",
                                "description": "Whether to use browser cookies (defaults to false to avoid OS Keychain prompts and slowdowns)",
                                "default": false
                            },
                            "browser": {
                                "type": "string",
                                "description": "Override the browser profile used to resolve cookies and user agent",
                                "enum": ["firefox", "chrome", "edge", "safari", "brave"]
                            }
                        },
                        "required": ["tool"]
                    }).as_object().expect("Schema object").clone()),
                    output_schema: None,
                    icons: None,
                }
            ],
            next_cursor: None,

        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let args = request.arguments.unwrap_or_default();

        match request.name.as_ref() {
            "scrape_url" | "get" => {
                let url = args
                    .get("url")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'url' parameter".to_string())
                    })?;

                let use_cookies = args
                    .get("use_cookies")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);

                let output_format = ingest::output_format_from_args(&args)?;

                let browser = self
                    .resolve_browser_override(args.get("browser").and_then(|v| v.as_str()))
                    .await?;

                let domain = get_domain(url).map_err(|e| ConnectorError::Other(e.to_string()))?;
                let cookies = self
                    .cookies_for_request(&browser, &domain, use_cookies)
                    .await?;

                debug!(
                    target = "web.scrape_url",
                    %url,
                    use_cookies,
                    browser = browser_identifier(&browser),
                    "executing scrape"
                );

                let content = self.scrape_url(url, &browser, cookies.as_deref()).await?;

                if output_format == OutputFormat::NormalizedV1 {
                    let item = web_item_from_content(&content);
                    let normalized = NormalizedItemV1::new(
                        item,
                        Partial::complete(None),
                        Source::new("web", request.name.as_ref()),
                    );
                    return crate::utils::structured_result(&normalized);
                }

                let text = serde_json::to_string(&content)?;
                Ok(CallToolResult::success(text.into_contents()))
            }
            "scrape_with_config" => {
                let tool_obj = match args.get("tool").and_then(|v| v.as_object()) {
                    Some(tool) => tool,
                    None => {
                        return Err(ConnectorError::InvalidParams(
                            "Missing or invalid tool parameter".to_string(),
                        ))
                    }
                };

                let parameters = args
                    .get("parameters")
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default();

                let tool_config: ToolConfig =
                    serde_json::from_value(serde_json::Value::Object(tool_obj.clone()))?;

                let url = self.process_url_template(
                    &tool_config.url_template,
                    &tool_config.url_parameters,
                    &parameters,
                )?;

                let browser = self
                    .resolve_browser_override(args.get("browser").and_then(|v| v.as_str()))
                    .await?;

                let use_cookies = args
                    .get("use_cookies")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);

                let domain = get_domain(&url).map_err(|e| ConnectorError::Other(e.to_string()))?;
                let cookies = self
                    .cookies_for_request(&browser, &domain, use_cookies)
                    .await?;

                debug!(
                    target = "web.scrape_with_config",
                    url = %url,
                    use_cookies,
                    browser = browser_identifier(&browser),
                    "executing scrape"
                );

                let content = self.scrape_url(&url, &browser, cookies.as_deref()).await?;

                let html = content.content.clone();

                let document = Html::parse_document(&html);

                let mut results = Vec::new();

                if let Some(item_selector_str) = &tool_config.item_selector {
                    // Multiple items case
                    let item_selector = match scraper::Selector::parse(item_selector_str) {
                        Ok(selector) => selector,
                        Err(e) => {
                            info!("Error parsing item selector: {}", e);
                            return Ok(CallToolResult::error(
                                "Error parsing item selector".to_string().into_contents(),
                            ));
                        }
                    };

                    for item in document.select(&item_selector) {
                        let mut item_data = HashMap::new();
                        for selector_config in &tool_config.selectors {
                            let field_selector =
                                match scraper::Selector::parse(&selector_config.selector) {
                                    Ok(selector) => selector,
                                    Err(e) => {
                                        info!(
                                            "Error parsing field selector '{}': {}",
                                            selector_config.selector, e
                                        );
                                        continue;
                                    }
                                };

                            let value = if let Some(attr) = &selector_config.attribute {
                                item.select(&field_selector)
                                    .next()
                                    .and_then(|el| el.value().attr(attr))
                                    .map(|s| s.to_string())
                                    .unwrap_or_default()
                            } else {
                                item.select(&field_selector)
                                    .next()
                                    .map(|el| {
                                        el.text().collect::<Vec<_>>().join(" ").trim().to_string()
                                    })
                                    .unwrap_or_default()
                            };
                            item_data.insert(selector_config.name.clone(), value);
                        }
                        results.push(item_data);
                    }
                } else {
                    // Single item case
                    let mut single_result = HashMap::new();
                    for selector_config in &tool_config.selectors {
                        let field_selector =
                            match scraper::Selector::parse(&selector_config.selector) {
                                Ok(selector) => selector,
                                Err(e) => {
                                    info!(
                                        "Error parsing field selector '{}': {}",
                                        selector_config.selector, e
                                    );
                                    continue;
                                }
                            };

                        let value = if let Some(attr) = &selector_config.attribute {
                            document
                                .select(&field_selector)
                                .next()
                                .and_then(|el| el.value().attr(attr))
                                .map(|s| s.to_string())
                                .unwrap_or_default()
                        } else {
                            document
                                .select(&field_selector)
                                .next()
                                .map(|el| {
                                    el.text().collect::<Vec<_>>().join(" ").trim().to_string()
                                })
                                .unwrap_or_default()
                        };
                        single_result.insert(selector_config.name.clone(), value);
                    }
                    results.push(single_result);
                }

                // Apply post-processing to each item
                for item in &mut results {
                    for (field, value) in item.iter_mut() {
                        if let Some(selector_config) =
                            tool_config.selectors.iter().find(|s| s.name == *field)
                        {
                            if let Some(post_processing) = &selector_config.post_processing {
                                for process in post_processing {
                                    match process.as_str() {
                                        "trim" => *value = value.trim().to_string(),
                                        "lowercase" => *value = value.to_lowercase(),
                                        "uppercase" => *value = value.to_uppercase(),
                                        _ => {
                                            info!("Unknown post-processing: {}", process);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let text = serde_json::to_string(&results)?;
                Ok(CallToolResult::success(text.into_contents()))
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        // Implement initialization logic (if needed).
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                version: "0.1.0".to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some("MCP connector for various data sources".to_string()),
        })
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let _cursor = request.and_then(|r| r.cursor);
        let resources = vec![];

        Ok(ListResourcesResult {
            resources,
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        let _uri_str = request.uri.as_str();

        Ok(vec![])
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        let _cursor = request.and_then(|r| r.cursor);
        let prompts = vec![];
        Ok(ListPromptsResult {
            prompts,
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::ToolNotFound)
    }
}

pub fn find_main_content(html: &Html) -> String {
    // Try common content selectors in order of likelihood
    let selectors = [
        // Common article content selectors
        "article",
        "main",
        ".post-content",
        "#post_content",
        ".article-content",
        ".entry-content",
        ".content-area",
        ".main-content",
        ".post-body",
        ".article__body",
        ".dp-container",
        ".a-container",
        // Specific to some websites
        ".post__content",
        "[itemprop='articleBody']",
        ".story-body",
        ".story__body",
        // Fallbacks
        ".content",
        "#content",
        ".container",
        ".page-content",
    ];

    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = html.select(&selector).next() {
                return element.html();
            }
        }
    }

    // If no content selectors match, try to find the element with the most text content
    // This is a simplified version of the content density heuristic used by readability algorithms
    match Selector::parse("body") {
        Ok(body_selector) => {
            if let Some(element) = html.select(&body_selector).next() {
                return element.html();
            }
        }
        Err(_) => {
            // Try html as fallback
            if let Ok(html_selector) = Selector::parse("html") {
                if let Some(element) = html.select(&html_selector).next() {
                    return element.html();
                }
            }
        }
    }

    // Return empty string if no suitable element found
    String::new()
}

pub fn html_to_markdown(html: &str) -> String {
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "nav", "footer", "header", "aside", "img", "a", "href", "src",
        ])
        .build();
    converter.convert(html).unwrap_or_else(|_| html.to_string())
}
