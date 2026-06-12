use async_trait::async_trait;
use rmcp::model::*;
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::RwLock;
use url::Url;

use crate::auth::AuthDetails;
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;

const BWT_API_BASE: &str = "https://ssl.bing.com/webmaster/api.svc/json";
const INDEXNOW_API_ENDPOINT: &str = "https://api.indexnow.org/indexnow";

pub struct BingWebmasterToolsConnector {
    client: reqwest::Client,
    auth: RwLock<AuthDetails>,
}

impl BingWebmasterToolsConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = reqwest::Client::builder()
            .user_agent("rzn-tools/0.2 (bing-webmaster-tools)")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        Ok(Self {
            client,
            auth: RwLock::new(auth),
        })
    }

    async fn auth_snapshot(&self) -> AuthDetails {
        self.auth.read().await.clone()
    }

    async fn api_key(&self) -> Result<String, ConnectorError> {
        let auth = self.auth_snapshot().await;
        auth.get("api_key")
            .cloned()
            .or_else(|| std::env::var("BING_WEBMASTER_API_KEY").ok())
            .ok_or_else(|| {
                ConnectorError::Authentication(
                    "Missing api_key. Run `rzn-tools setup bing-webmaster-tools` or set BING_WEBMASTER_API_KEY."
                        .to_string(),
                )
            })
    }

    async fn indexnow_key(&self) -> Option<String> {
        let auth = self.auth_snapshot().await;
        auth.get("indexnow_key")
            .cloned()
            .or_else(|| std::env::var("INDEXNOW_KEY").ok())
            .or_else(|| std::env::var("BING_INDEXNOW_KEY").ok())
    }

    async fn indexnow_key_location(&self) -> Option<String> {
        let auth = self.auth_snapshot().await;
        auth.get("indexnow_key_location")
            .cloned()
            .or_else(|| std::env::var("INDEXNOW_KEY_LOCATION").ok())
            .or_else(|| std::env::var("BING_INDEXNOW_KEY_LOCATION").ok())
    }

    fn host_from_url(url: &str) -> Result<String, ConnectorError> {
        let parsed = Url::parse(url)
            .map_err(|e| ConnectorError::InvalidParams(format!("Invalid URL `{}`: {}", url, e)))?;
        parsed
            .host_str()
            .map(|h| h.to_string())
            .ok_or_else(|| ConnectorError::InvalidParams(format!("URL missing host: `{}`", url)))
    }

    async fn indexnow_post(&self, body: &Value) -> Result<Value, ConnectorError> {
        let resp = self
            .client
            .post(INDEXNOW_API_ENDPOINT)
            .json(body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        if !status.is_success() {
            let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
            return Err(ConnectorError::Other(format!(
                "IndexNow API error {}: {}",
                status, v
            )));
        }
        if text.trim().is_empty() {
            return Ok(json!({ "success": true, "status": status.as_u16() }));
        }
        Ok(serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text })))
    }

    async fn indexnow_get(
        &self,
        url: &str,
        key: &str,
        key_location: Option<&str>,
    ) -> Result<Value, ConnectorError> {
        let mut qp: Vec<(&str, String)> = vec![("url", url.to_string()), ("key", key.to_string())];
        if let Some(kl) = key_location {
            qp.push(("keyLocation", kl.to_string()));
        }
        let resp = self
            .client
            .get(INDEXNOW_API_ENDPOINT)
            .query(&qp)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        if !status.is_success() {
            let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
            return Err(ConnectorError::Other(format!(
                "IndexNow API error {}: {}",
                status, v
            )));
        }
        if text.trim().is_empty() {
            return Ok(json!({ "success": true, "status": status.as_u16() }));
        }
        Ok(serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text })))
    }

    async fn bwt_get(
        &self,
        method_name: &str,
        params: &[(&str, String)],
    ) -> Result<Value, ConnectorError> {
        let api_key = self.api_key().await?;
        let url = format!("{}/{}", BWT_API_BASE, method_name);
        let mut qp: Vec<(&str, String)> = Vec::with_capacity(params.len() + 1);
        qp.push(("apikey", api_key));
        qp.extend_from_slice(params);

        let resp = self
            .client
            .get(&url)
            .query(&qp)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Bing Webmaster Tools API error {}: {}",
                status, v
            )));
        }
        Ok(v)
    }

    async fn bwt_post(&self, method_name: &str, body: &Value) -> Result<Value, ConnectorError> {
        let api_key = self.api_key().await?;
        let url = format!("{}/{}", BWT_API_BASE, method_name);
        let resp = self
            .client
            .post(&url)
            .query(&[("apikey", api_key)])
            .json(body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Bing Webmaster Tools API error {}: {}",
                status, v
            )));
        }
        Ok(v)
    }

    fn unwrap_d_if_present(resp: &Value) -> Value {
        resp.get("d").cloned().unwrap_or_else(|| resp.clone())
    }
}

#[async_trait]
impl Connector for BingWebmasterToolsConnector {
    fn name(&self) -> &'static str {
        "bing-webmaster-tools"
    }

    fn description(&self) -> &'static str {
        "Bing Webmaster Tools API connector for site list, performance/crawl stats, issues, and URL submission."
    }

    fn display_name(&self) -> &'static str {
        "Bing Webmaster Tools"
    }

    fn icon(&self) -> &'static str {
        "bing"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["seo", "analytics", "search"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: Some(Default::default()),
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                title: None,
                version: "0.1.0".to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Configure an API key from Bing Webmaster Tools (Settings → API Access). Prefer `rzn-tools setup bing-webmaster-tools` or set BING_WEBMASTER_API_KEY. Optional: configure IndexNow via indexnow_key/INDEXNOW_KEY (and host the key file at https://<host>/<key>.txt)."
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: vec![],
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
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("list_sites"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List sites available in Bing Webmaster Tools for the authenticated user.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_rank_and_traffic_stats"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get rank and traffic stats for a site (daily updated).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_crawl_stats"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get crawl statistics for a site (Bingbot crawl activity).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_crawl_issues"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get crawl issues for a site (Bing crawl problems).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_keyword_data"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get keyword research data from Bing (volume + related keywords).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Keyword or phrase to research." },
                            "country": { "type": "string", "default": "us", "description": "Country code (e.g., us, gb, de)." },
                            "language": { "type": "string", "default": "en", "description": "Language code (e.g., en, de, fr)." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["query"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_backlinks"),
                title: None,
                description: Some(Cow::Borrowed("Get backlink/link count data for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "page": { "type": "integer", "minimum": 0, "default": 0, "description": "Pagination page number (default: 0)." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_query_stats"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get query stats for a site (weekly updated).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_query_traffic_stats"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get traffic stats for a specific query on a site (daily updated).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "query": { "type": "string" },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url", "query"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_page_stats"),
                title: None,
                description: Some(Cow::Borrowed("Get page stats for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_url_submission_quota"),
                title: None,
                description: Some(Cow::Borrowed("Get URL submission quota for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("submit_url"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Submit a single URL for indexing (URL Submission API).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "url": { "type": "string", "description": "The specific URL to submit for indexing." }
                        },
                        "required": ["site_url", "url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("submit_url_batch"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Submit a batch of URLs for indexing (URL Submission API).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "url_list": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "URLs to submit for indexing." }
                        },
                        "required": ["site_url", "url_list"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_url_info"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get details for a specific URL (index status / issues).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "url": { "type": "string", "description": "The specific URL to fetch info for." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url", "url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_deep_links"),
                title: None,
                description: Some(Cow::Borrowed("Get deep links (sitelinks) for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_blocked_urls"),
                title: None,
                description: Some(Cow::Borrowed("List URLs blocked by robots.txt for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_query_page_stats"),
                title: None,
                description: Some(Cow::Borrowed("Get combined query+page stats for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("add_site"),
                title: None,
                description: Some(Cow::Borrowed("Add a site to Bing Webmaster Tools.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "The site URL to add (must be a valid URL)." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("verify_site"),
                title: None,
                description: Some(Cow::Borrowed("Get verification status for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "The site URL to check verification for." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_content_issues"),
                title: None,
                description: Some(Cow::Borrowed("Get content-related SEO issues for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_malware_issues"),
                title: None,
                description: Some(Cow::Borrowed("Get malware/security issues for a site.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Your site URL as registered in Bing Webmaster Tools." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("indexnow_submit_url"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Submit a URL via IndexNow (fast indexing). Requires an IndexNow key hosted at `https://<host>/<key>.txt`.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "url": { "type": "string", "description": "The URL to submit for indexing." },
                            "host": { "type": "string", "description": "Optional host override. If omitted, derived from url." },
                            "key": { "type": "string", "description": "Optional IndexNow key override. If omitted, uses connector auth/indexnow_key or env INDEXNOW_KEY." },
                            "key_location": { "type": "string", "description": "Optional key location URL override. If omitted, defaults to https://<host>/<key>.txt." }
                        },
                        "required": ["url"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("indexnow_submit_url_batch"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Submit multiple URLs via IndexNow (batch). Requires an IndexNow key hosted at `https://<host>/<key>.txt`.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "url_list": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "URLs to submit for indexing." },
                            "host": { "type": "string", "description": "Optional host override. If omitted, derived from urls (must share a host)." },
                            "key": { "type": "string", "description": "Optional IndexNow key override. If omitted, uses connector auth/indexnow_key or env INDEXNOW_KEY." },
                            "key_location": { "type": "string", "description": "Optional key location URL override. If omitted, defaults to https://<host>/<key>.txt." }
                        },
                        "required": ["url_list"],
                        "additionalProperties": false
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
        ];

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let args = request.arguments.unwrap_or_default();
        let concise = !matches!(
            args.get("response_format").and_then(|v| v.as_str()),
            Some("detailed")
        );

        match request.name.as_ref() {
            "list_sites" => {
                let resp = self.bwt_get("GetUserSites", &[]).await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_rank_and_traffic_stats" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get(
                        "GetRankAndTrafficStats",
                        &[("siteUrl", site_url.to_string())],
                    )
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_crawl_stats" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetCrawlStats", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_crawl_issues" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetCrawlIssues", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_keyword_data" => {
                let query = args.get("query").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("query is required".to_string()),
                )?;
                let country = args.get("country").and_then(|v| v.as_str()).unwrap_or("us");
                let language = args
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("en");
                let resp = self
                    .bwt_get(
                        "GetKeywordData",
                        &[
                            ("q", query.to_string()),
                            ("country", country.to_string()),
                            ("language", language.to_string()),
                        ],
                    )
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_backlinks" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let page = args
                    .get("page")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0)
                    .max(0);
                let resp = self
                    .bwt_get(
                        "GetLinkCounts",
                        &[
                            ("siteUrl", site_url.to_string()),
                            ("page", page.to_string()),
                        ],
                    )
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_query_stats" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetQueryStats", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_query_traffic_stats" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let query = args.get("query").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("query is required".to_string()),
                )?;
                let resp = self
                    .bwt_get(
                        "GetQueryTrafficStats",
                        &[
                            ("siteUrl", site_url.to_string()),
                            ("query", query.to_string()),
                        ],
                    )
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_page_stats" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetPageStats", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_url_submission_quota" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get(
                        "GetUrlSubmissionQuota",
                        &[("siteUrl", site_url.to_string())],
                    )
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "submit_url" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or(ConnectorError::InvalidParams("url is required".to_string()))?;
                let mut body = Map::new();
                body.insert("siteUrl".to_string(), json!(site_url));
                body.insert("url".to_string(), json!(url));
                let resp = self.bwt_post("SubmitUrl", &Value::Object(body)).await?;
                structured_result_with_text(&resp, None)
            }
            "submit_url_batch" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let url_list = args.get("url_list").and_then(|v| v.as_array()).ok_or(
                    ConnectorError::InvalidParams("url_list must be an array".to_string()),
                )?;
                let urls: Vec<String> = url_list
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                if urls.is_empty() {
                    return Err(ConnectorError::InvalidParams(
                        "url_list must contain at least one URL".to_string(),
                    ));
                }
                let mut body = Map::new();
                body.insert("siteUrl".to_string(), json!(site_url));
                body.insert("urlList".to_string(), json!(urls));
                let resp = self
                    .bwt_post("SubmitUrlBatch", &Value::Object(body))
                    .await?;
                structured_result_with_text(&resp, None)
            }
            "get_url_info" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or(ConnectorError::InvalidParams("url is required".to_string()))?;
                let resp = self
                    .bwt_get(
                        "GetUrlInfo",
                        &[("siteUrl", site_url.to_string()), ("url", url.to_string())],
                    )
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_deep_links" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetDeepLinks", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_blocked_urls" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetBlockedUrls", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_query_page_stats" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetQueryPageStats", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "add_site" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("AddSite", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "verify_site" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get(
                        "GetVerificationStatus",
                        &[("siteUrl", site_url.to_string())],
                    )
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_content_issues" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetContentIssues", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "get_malware_issues" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let resp = self
                    .bwt_get("GetMalwareIssues", &[("siteUrl", site_url.to_string())])
                    .await?;
                if concise {
                    return structured_result_with_text(&Self::unwrap_d_if_present(&resp), None);
                }
                structured_result_with_text(&resp, None)
            }
            "indexnow_submit_url" => {
                let url = args
                    .get("url")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ConnectorError::InvalidParams("url is required".to_string()))?;
                let host = if let Some(h) = args.get("host").and_then(|v| v.as_str()) {
                    h.to_string()
                } else {
                    Self::host_from_url(url)?
                };
                let key = if let Some(k) = args.get("key").and_then(|v| v.as_str()) {
                    k.to_string()
                } else {
                    self.indexnow_key().await.ok_or_else(|| {
                        ConnectorError::Authentication(
                            "Missing IndexNow key. Add `indexnow_key` to bing-webmaster-tools auth, or set INDEXNOW_KEY."
                                .to_string(),
                        )
                    })?
                };
                let key_location =
                    if let Some(kl) = args.get("key_location").and_then(|v| v.as_str()) {
                        kl.to_string()
                    } else if let Some(kl) = self.indexnow_key_location().await {
                        kl
                    } else {
                        format!("https://{}/{}.txt", host, key)
                    };

                let resp = self.indexnow_get(url, &key, Some(&key_location)).await?;
                structured_result_with_text(
                    &json!({
                        "request": { "host": host, "key_location": key_location, "url": url },
                        "response": resp
                    }),
                    None,
                )
            }
            "indexnow_submit_url_batch" => {
                let url_list = args.get("url_list").and_then(|v| v.as_array()).ok_or(
                    ConnectorError::InvalidParams("url_list must be an array".to_string()),
                )?;
                let urls: Vec<String> = url_list
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                if urls.is_empty() {
                    return Err(ConnectorError::InvalidParams(
                        "url_list must contain at least one URL".to_string(),
                    ));
                }

                let host = if let Some(h) = args.get("host").and_then(|v| v.as_str()) {
                    h.to_string()
                } else {
                    let first = Self::host_from_url(&urls[0])?;
                    for u in &urls[1..] {
                        let h = Self::host_from_url(u)?;
                        if h != first {
                            return Err(ConnectorError::InvalidParams(
                                "All URLs must share the same host (or pass host explicitly)"
                                    .to_string(),
                            ));
                        }
                    }
                    first
                };

                let key = if let Some(k) = args.get("key").and_then(|v| v.as_str()) {
                    k.to_string()
                } else {
                    self.indexnow_key().await.ok_or_else(|| {
                        ConnectorError::Authentication(
                            "Missing IndexNow key. Add `indexnow_key` to bing-webmaster-tools auth, or set INDEXNOW_KEY."
                                .to_string(),
                        )
                    })?
                };
                let key_location =
                    if let Some(kl) = args.get("key_location").and_then(|v| v.as_str()) {
                        kl.to_string()
                    } else if let Some(kl) = self.indexnow_key_location().await {
                        kl
                    } else {
                        format!("https://{}/{}.txt", host, key)
                    };

                let body = json!({
                    "host": host,
                    "key": key,
                    "keyLocation": key_location,
                    "urlList": urls
                });
                let resp = self.indexnow_post(&body).await?;
                let redacted_request = json!({
                    "host": body.get("host").cloned().unwrap_or(Value::Null),
                    "keyLocation": body.get("keyLocation").cloned().unwrap_or(Value::Null),
                    "urlList": body.get("urlList").cloned().unwrap_or(Value::Null)
                });
                structured_result_with_text(
                    &json!({ "request": redacted_request, "response": resp }),
                    None,
                )
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(
            "Prompt not found".to_string(),
        ))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(self.auth_snapshot().await)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        let mut guard = self.auth.write().await;
        *guard = details;
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let auth = self.auth_snapshot().await;
        if auth.contains_key("api_key")
            || std::env::var("BING_WEBMASTER_API_KEY").is_ok()
            || auth.contains_key("indexnow_key")
            || std::env::var("INDEXNOW_KEY").is_ok()
            || std::env::var("BING_INDEXNOW_KEY").is_ok()
        {
            return Ok(());
        }
        Err(ConnectorError::Authentication(
            "Bing Webmaster Tools auth not configured (api_key and/or indexnow_key missing)"
                .to_string(),
        ))
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "api_key".to_string(),
                    label: "Bing Webmaster Tools API Key".to_string(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some(
                        "Create an API key in Bing Webmaster Tools → Settings → API Access. Or set BING_WEBMASTER_API_KEY."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "indexnow_key".to_string(),
                    label: "IndexNow Key (optional)".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Used for IndexNow submissions (fast indexing). Also settable via INDEXNOW_KEY."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "indexnow_key_location".to_string(),
                    label: "IndexNow Key Location (optional)".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Public URL of the key file (defaults to https://<host>/<key>.txt). Also settable via INDEXNOW_KEY_LOCATION."
                            .to_string(),
                    ),
                    options: None,
                },
            ],
        }
    }
}
