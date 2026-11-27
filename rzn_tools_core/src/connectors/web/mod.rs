use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::http::{Egress, HttpClient};
use crate::ingest::{self, OutputFormat};
#[cfg(feature = "browser-cookie-import")]
use crate::utils::get_cookies;
use crate::utils::{get_domain, get_user_agent, match_browser, strip_multiple_newlines, Browser};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
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
    /// SHA-256 of the fetched HTML, the page's change marker.
    #[serde(skip)]
    pub raw_sha256: String,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct WebMetadata {
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub author: Option<String>,
    pub published_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
}

impl From<&page::PageFields> for WebMetadata {
    fn from(fields: &page::PageFields) -> Self {
        Self {
            description: fields.description.clone(),
            keywords: fields.keywords.clone(),
            author: fields.author.clone(),
            published_date: fields.published.clone(),
            modified_date: fields.modified.clone(),
            site_name: fields.site_name.clone(),
            image: fields.image.clone(),
            canonical: fields.canonical.clone(),
        }
    }
}

mod discover;
mod fetch;
mod item_v2;
mod page;
#[cfg(test)]
mod served_tests;

/// One page fetch, read: either the page or a 304 to the validators it was asked with.
enum PageFetch {
    NotModified(fetch::Validators),
    Page(Box<WebContent>, Box<item_v2::FetchedPage>),
}

#[derive(Clone)]
pub struct WebConnector {
    client: HttpClient,
    pub headers: HeaderMap,
    pub browser: Browser,
    cookie_cache: Arc<Mutex<HashMap<String, String>>>,
    /// The gap between two requests to one host.
    gap: Duration,
}

impl WebConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let browser = match_browser(
            auth.get("browser")
                .unwrap_or(&"firefox".to_string())
                .to_string(),
        )
        .await?;

        let client = HttpClient::new(Egress::from_auth(&auth)?, |b| {
            b.http1_only()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(20))
                .pool_max_idle_per_host(2)
                .tcp_keepalive(Some(Duration::from_secs(30)))
                .user_agent(DEFAULT_USER_AGENT)
        })?;

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
            gap: fetch::HOST_GAP,
        };

        connector.set_auth_details(auth).await?;
        Ok(connector)
    }

    /// The request headers for one fetch: the user agent and, when asked for, the cookies.
    fn request_headers(
        &self,
        browser: &Browser,
        cookies: Option<&str>,
    ) -> Result<HeaderMap, ConnectorError> {
        let user_agent = self
            .headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(|ua| ua.to_string())
            .unwrap_or_else(|| get_user_agent(browser.clone()));
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
        headers.insert(
            USER_AGENT,
            HeaderValue::from_str(&user_agent).map_err(|e| ConnectorError::Other(e.to_string()))?,
        );
        if let Some(cookie_header) = cookies.filter(|c| !c.is_empty()) {
            headers.insert(
                COOKIE,
                HeaderValue::from_str(cookie_header)
                    .map_err(|e| ConnectorError::Other(e.to_string()))?,
            );
        }
        Ok(headers)
    }

    /// Fetch one page once, paced, with `validators`; a block (403, 429 after back-off, a bot
    /// wall) or any other failed status is an error naming it.
    async fn fetch_page(
        &self,
        url: &str,
        browser: &Browser,
        cookies: Option<&str>,
        validators: &fetch::Validators,
    ) -> Result<PageFetch, ConnectorError> {
        let headers = self.request_headers(browser, cookies)?;
        let fetched = fetch::get(&self.client, url, &headers, validators, self.gap).await?;
        if fetched.status == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(PageFetch::NotModified(fetch::Validators {
                etag: fetched.validators.etag.or_else(|| validators.etag.clone()),
                last_modified: fetched
                    .validators
                    .last_modified
                    .or_else(|| validators.last_modified.clone()),
            }));
        }
        let text = String::from_utf8_lossy(&fetched.body).into_owned();
        if let Some(reason) = page::blocked(fetched.status, &text) {
            return Err(ConnectorError::Other(format!("{reason} from {url}")));
        }
        if !fetched.status.is_success() {
            return Err(ConnectorError::Other(format!(
                "HTTP {} from {url}",
                fetched.status.as_u16()
            )));
        }
        let media_type = fetched
            .content_type
            .as_deref()
            .and_then(|value| value.split(';').next())
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "text/html".to_string());
        let base = url::Url::parse(&fetched.final_url)
            .map_err(|e| ConnectorError::Other(format!("final URL of {url}: {e}")))?;
        let (main_text, fields, shell) = if media_type.contains("html") {
            let html = Html::parse_document(&text);
            let main_text = strip_multiple_newlines(&html_to_markdown(&find_main_content(&html)));
            let fields = page::read_fields(&html, &base);
            let shell = page::empty_shell(&html, page::words(&main_text));
            (main_text, fields, shell)
        } else {
            let fields = page::PageFields {
                title: text
                    .lines()
                    .find_map(|line| line.strip_prefix("# "))
                    .map(|title| title.trim().to_string()),
                ..Default::default()
            };
            (text.clone(), fields, None)
        };
        debug!(
            target: "connector.web",
            url = %url,
            status = %fetched.status.as_u16(),
            body_bytes = %fetched.body.len(),
            main_words = %page::words(&main_text),
            "fetched page"
        );
        let raw_sha256 = {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(&fetched.body))
        };
        let content = WebContent {
            url: fetched.final_url.clone(),
            title: fields.title.clone(),
            content: main_text.clone(),
            metadata: WebMetadata::from(&fields),
            raw_sha256,
        };
        let page = item_v2::FetchedPage {
            key: item_v2::normalize_url(url)?,
            final_url: fetched.final_url,
            fields,
            main_text,
            media_type,
            body: fetched.body,
            validators: fetched.validators,
            shell,
        };
        Ok(PageFetch::Page(Box::new(content), Box::new(page)))
    }

    async fn scrape_url(
        &self,
        url: &str,
        browser: &Browser,
        cookies: Option<&str>,
    ) -> Result<WebContent, ConnectorError> {
        match self
            .fetch_page(url, browser, cookies, &fetch::Validators::default())
            .await?
        {
            PageFetch::Page(content, _) => Ok(*content),
            PageFetch::NotModified(_) => Err(ConnectorError::Other(format!(
                "{url} answered 304 to a request without validators"
            ))),
        }
    }

    /// A listing document (robots.txt, a sitemap, a feed, an llms.txt), paced like a page; any
    /// status but success is an error.
    async fn fetch_text(&self, url: &str) -> Result<String, ConnectorError> {
        self.fetch_optional(url)
            .await?
            .ok_or_else(|| ConnectorError::Other(format!("HTTP 404 from {url}")))
    }

    /// As [`Self::fetch_text`], but a 404 or 410 is `None`.
    async fn fetch_optional(&self, url: &str) -> Result<Option<String>, ConnectorError> {
        let headers = self.request_headers(&self.browser.clone(), None)?;
        let fetched = fetch::get(
            &self.client,
            url,
            &headers,
            &fetch::Validators::default(),
            self.gap,
        )
        .await?;
        let status = fetched.status;
        if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&fetched.body).into_owned();
        if let Some(reason) = page::blocked(status, &text) {
            return Err(ConnectorError::Other(format!("{reason} from {url}")));
        }
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "HTTP {} from {url}",
                status.as_u16()
            )));
        }
        Ok(Some(text))
    }

    /// Pages to index from one scope, as `rzn.items/v2`.
    async fn list_pages(
        &self,
        args: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        use crate::item::Listing;
        let text = |name: &str| args.get(name).and_then(|v| v.as_str());
        let page = if let Some(urls) = args.get("urls").and_then(|v| v.as_array()) {
            let urls: Vec<String> = urls
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            item_v2::url_list_page(&urls)?
        } else if let Some(sitemap) = text("sitemap") {
            let entries = self.sitemap_entries(vec![sitemap.to_string()]).await?;
            item_v2::pages_page(&entries, Listing::Full)?
        } else if let Some(feed) = text("feed") {
            self.feed_listing(feed).await?
        } else if let Some(site) = text("site") {
            item_v2::pages_page(&self.site_entries(site).await?, Listing::Full)?
        } else if let Some(llms) = text("llms_txt") {
            let urls = self.llms_listing(llms).await?;
            item_v2::url_list_page(&urls)?
        } else {
            return Err(ConnectorError::InvalidParams(
                "list_pages needs one of urls, sitemap, feed, site or llms_txt".to_string(),
            ));
        };
        crate::utils::structured_result(&page)
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
        _browser: &Browser,
        _domain: &str,
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

        #[cfg(not(feature = "browser-cookie-import"))]
        {
            Err(ConnectorError::InvalidInput(
                "Automatic browser cookie import requires the browser-cookie-import feature; supply a Cookie header or use rzn-browser instead".to_string(),
            ))
        }

        #[cfg(feature = "browser-cookie-import")]
        {
            let cache_key = format!("{}|{}", browser_identifier(_browser), _domain);
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
                get_cookies(_browser.clone(), _domain.to_string()),
            )
            .await;

            let cookies = match cookies_res {
                Ok(Ok(c)) => c,
                Ok(Err(e)) => {
                    debug!(
                        target: "connector.web",
                    domain = %_domain,
                        error = %e,
                        "cookie extraction failed; continuing without cookies"
                    );
                    return Ok(None);
                }
                Err(_) => {
                    debug!(
                        target: "connector.web",
                    domain = %_domain,
                        "cookie extraction timed out; continuing without cookies"
                    );
                    return Ok(None);
                }
            };
            let t1 = std::time::Instant::now();

            debug!(
                target: "connector.web",
            domain = %_domain,
                ms = %((t1 - t0).as_millis()),
                "retrieved cookies from browser store"
            );

            {
                let mut cache = self.cookie_cache.lock().await;
                cache.insert(cache_key, cookies.clone());
            }

            Ok(Some(cookies))
        }
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

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.client.apply_auth(&details)?;
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
                        "Which browser profile to import cookies from when browser-cookie-import is enabled.".into(),
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
                                "description": "Whether to use an explicitly configured Cookie header. Automatic browser import requires browser-cookie-import.",
                                "default": false
                            },
                            "if_none_match": {"type": "string", "description": "The ETag of the last fetch; a 304 returns an item marked not_modified."},
                            "if_modified_since": {"type": "string", "description": "The Last-Modified of the last fetch."},
                        "browser": {
                            "type": "string",
                            "description": "Override the browser identity used for user-agent selection; automatic cookie import requires browser-cookie-import.",
                            "enum": ["firefox", "chrome", "edge", "safari", "brave"],
                            "default": "firefox"
                        },
                        "output_format": {
                            "type": "string",
                            "enum": ["raw", "item_v2", "display_v1"],
                            "default": "raw",
                            "description": "Default raw. Use item_v2 for ingestion pipelines. Use display_v1 for UI-friendly output."
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
                                "description": "Whether to use an explicitly configured Cookie header. Automatic browser import requires browser-cookie-import.",
                                "default": false
                            },
                            "browser": {
                                "type": "string",
                            "description": "Override the browser identity used for user-agent selection; automatic cookie import requires browser-cookie-import.",
                                "enum": ["firefox", "chrome", "edge", "safari", "brave"]
                            }
                        },
                        "required": ["tool"]
                    }).as_object().expect("Schema object").clone()),
                    output_schema: None,
                    icons: None,
                },
                Tool {
                    name: Cow::Borrowed("list_pages"),
                    title: Some("List Pages For Ingestion".into()),
                    description: Some(Cow::Borrowed(
                        "List pages to index from exactly one scope: `urls` (a list), `sitemap` (a sitemap or sitemap index URL), `feed` (an RSS or Atom feed, or a page that links one), `site` (a site's robots.txt sitemaps, else /sitemap.xml, plus its /llms.txt), or `llms_txt` (an llms.txt URL). Returns rzn.items/v2; fetch each with scrape_url output_format=item_v2. No link-following.",
                    )),
                    annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
                    input_schema: Arc::new(json!({
                        "type": "object",
                        "properties": {
                            "urls": {"type": "array", "items": {"type": "string"}},
                            "sitemap": {"type": "string"},
                            "feed": {"type": "string"},
                            "site": {"type": "string", "description": "A site's home URL."},
                            "llms_txt": {"type": "string"},
                            "limit": {"type": "integer", "description": "Ignored; a scope is listed whole."},
                            "cursor": {"type": "string", "description": "Ignored; one page."},
                            "output_format": crate::ingest::output_format_property()
                        },
                        "examples": [
                            {"description": "A sitemap", "input": {"sitemap": "https://example.com/sitemap.xml", "output_format": "item_v2"}}
                        ],
                        "_meta": {"category": "list", "tags": ["web"], "auth_required": false, "supports_output_format": true, "supports_cursor": false}
                    }).as_object().expect("Schema object").clone()),
                    output_schema: None,
                    icons: None,
                },
                Tool {
                    name: Cow::Borrowed("describe"),
                    title: Some("Describe For Ingestion".into()),
                    description: Some(Cow::Borrowed(
                        "What this connector gives an index: identity rule, listing type, scopes and fields. Returns rzn.describe/v2.",
                    )),
                    annotations: Some(ToolAnnotations::new().read_only(true).open_world(false)),
                    input_schema: Arc::new(json!({"type": "object", "properties": {}, "_meta": {"category": "describe", "supports_output_format": false}}).as_object().expect("Schema object").clone()),
                    output_schema: Some(Arc::new(crate::item::description_schema().as_object().cloned().unwrap_or_default())),
                    icons: None,
                },
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
            "describe" => crate::utils::structured_result(&item_v2::description()),
            "list_pages" => self.list_pages(&args).await,
            "scrape_url" => {
                // A web page's key is its normalized URL, so an index fetches by `item_ref`.
                let url = args
                    .get("url")
                    .or_else(|| args.get("item_ref"))
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

                let validators = fetch::Validators::from_args(&args);
                let fetched = self
                    .fetch_page(url, &browser, cookies.as_deref(), &validators)
                    .await?;
                match (fetched, output_format == OutputFormat::ItemV2) {
                    (PageFetch::NotModified(validators), true) => {
                        let key = item_v2::normalize_url(url)?;
                        crate::utils::structured_result(&item_v2::not_modified_item(
                            &key, validators,
                        ))
                    }
                    (PageFetch::Page(_, page), true) => {
                        crate::utils::structured_result(&item_v2::page_item(*page)?)
                    }
                    (PageFetch::NotModified(_), false) => Ok(CallToolResult::success(
                        "not modified".to_string().into_contents(),
                    )),
                    (PageFetch::Page(content, _), false) => {
                        let text = serde_json::to_string(&content)?;
                        Ok(CallToolResult::success(text.into_contents()))
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn explicit_cookie_header_is_used_without_browser_import() {
        let auth = AuthDetails::from_iter([("cookie".to_string(), "session=portable".to_string())]);
        let connector = WebConnector::new(auth).await.unwrap();

        let cookies = connector
            .cookies_for_request(&Browser::Firefox, "example.com", true)
            .await
            .unwrap();

        assert_eq!(cookies.as_deref(), Some("session=portable"));
    }

    #[cfg(not(feature = "browser-cookie-import"))]
    #[tokio::test]
    async fn automatic_cookie_import_is_rejected_when_not_compiled() {
        let connector = WebConnector::new(AuthDetails::new()).await.unwrap();

        let error = connector
            .cookies_for_request(&Browser::Firefox, "example.com", true)
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("browser-cookie-import"));
    }
}
