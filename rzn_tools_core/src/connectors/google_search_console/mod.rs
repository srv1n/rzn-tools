use async_trait::async_trait;
use rmcp::model::*;
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::oauth;
use crate::utils::structured_result_with_text;
use crate::Connector;

const GSC_API_BASE: &str = "https://www.googleapis.com/webmasters/v3";
const GSC_URL_INSPECTION_ENDPOINT: &str =
    "https://searchconsole.googleapis.com/v1/urlInspection/index:inspect";

pub struct GoogleSearchConsoleConnector {
    client: reqwest::Client,
    auth: RwLock<AuthDetails>,
}

impl GoogleSearchConsoleConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = reqwest::Client::builder()
            .user_agent("rzn-tools/0.2 (google-search-console)")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        Ok(Self {
            client,
            auth: RwLock::new(auth),
        })
    }

    fn now_epoch() -> i64 {
        chrono::Utc::now().timestamp()
    }

    async fn auth_snapshot(&self) -> AuthDetails {
        self.auth.read().await.clone()
    }

    async fn merged_auth(&self) -> AuthDetails {
        let in_mem = self.auth_snapshot().await;
        if in_mem.contains_key("access_token")
            || in_mem.contains_key("refresh_token")
            || in_mem.contains_key("client_id")
        {
            return in_mem;
        }
        let store = FileAuthStore::new_default();
        let mut stored = store
            .load(self.name())
            .or_else(|| store.load("google-common"))
            .unwrap_or_default();
        for (k, v) in in_mem.iter() {
            stored.insert(k.clone(), v.clone());
        }
        stored
    }

    async fn google_access_token(&self) -> Result<String, ConnectorError> {
        let auth = self.merged_auth().await;

        if let Some(access_token) = auth.get("access_token").cloned() {
            if let Some(exp_at) = auth.get("expires_at").and_then(|s| s.parse::<i64>().ok()) {
                if exp_at > Self::now_epoch() {
                    return Ok(access_token);
                }
            } else {
                // If expiry is absent, assume it's usable (caller will get 401 if not).
                return Ok(access_token);
            }
        }

        let refresh_token = auth.get("refresh_token").cloned().ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing refresh_token. Run `rzn-tools setup google-search-console`.".to_string(),
            )
        })?;
        let client_id = auth.get("client_id").cloned().ok_or_else(|| {
            ConnectorError::Authentication(
                "Missing client_id. Run `rzn-tools setup google-search-console`.".to_string(),
            )
        })?;
        let client_secret = auth.get("client_secret").cloned();

        let tokens =
            oauth::google_refresh_token(&client_id, client_secret.as_deref(), &refresh_token)
                .await?;
        let expires_at = tokens
            .expires_in
            .map(|exp| (Self::now_epoch() + exp - 60).to_string());

        let mut guard = self.auth.write().await;
        guard.insert("access_token".to_string(), tokens.access_token.clone());
        if let Some(rt) = tokens.refresh_token {
            guard.insert("refresh_token".to_string(), rt);
        }
        if let Some(exp_at) = expires_at {
            guard.insert("expires_at".to_string(), exp_at);
        }
        guard.entry("client_id".to_string()).or_insert(client_id);
        if let Some(cs) = client_secret {
            guard.entry("client_secret".to_string()).or_insert(cs);
        }

        Ok(tokens.access_token)
    }

    async fn gsc_get(&self, url: &str) -> Result<Value, ConnectorError> {
        let token = self.google_access_token().await?;
        let resp = self
            .client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Google Search Console API error {}: {}",
                status, v
            )));
        }
        Ok(v)
    }

    async fn gsc_post(&self, url: &str, body: &Value) -> Result<Value, ConnectorError> {
        let token = self.google_access_token().await?;
        let resp = self
            .client
            .post(url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Google Search Console API error {}: {}",
                status, v
            )));
        }
        Ok(v)
    }

    async fn gsc_put(&self, url: &str) -> Result<Value, ConnectorError> {
        let token = self.google_access_token().await?;
        let resp = self
            .client
            .put(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Google Search Console API error {}: {}",
                status, v
            )));
        }
        Ok(v)
    }

    async fn gsc_delete(&self, url: &str) -> Result<Value, ConnectorError> {
        let token = self.google_access_token().await?;
        let resp = self
            .client
            .delete(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let text = resp.text().await.map_err(ConnectorError::HttpRequest)?;
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({ "raw": text }));
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Google Search Console API error {}: {}",
                status, v
            )));
        }
        Ok(v)
    }
}

#[async_trait]
impl Connector for GoogleSearchConsoleConnector {
    fn name(&self) -> &'static str {
        "google-search-console"
    }

    fn description(&self) -> &'static str {
        "Google Search Console connector for site list, Search Analytics, sitemaps management, and URL inspection."
    }

    fn display_name(&self) -> &'static str {
        "Google Search Console"
    }

    fn icon(&self) -> &'static str {
        "google_search_console"
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
                "Authenticate via `rzn-tools setup google-search-console` (Google OAuth device flow). You must enable the Google Search Console API in Google Cloud and create an OAuth client (Desktop app recommended). Requires the `https://www.googleapis.com/auth/webmasters` scope for URL inspection."
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
                    "List Search Console properties (sites) accessible to the authenticated user.",
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
                name: Cow::Borrowed("get_site"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get details for a Search Console property (permission level, etc.).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Property URL, e.g. https://example.com/ or sc-domain:example.com" },
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
                name: Cow::Borrowed("search_analytics"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Run a Search Analytics query for a property (clicks, impressions, CTR, position).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Property URL, e.g. https://example.com/ or sc-domain:example.com" },
                            "start_date": { "type": "string", "description": "YYYY-MM-DD (interpreted in PT per the Search Console API)." },
                            "end_date": { "type": "string", "description": "YYYY-MM-DD (interpreted in PT per the Search Console API)." },
                            "dimensions": { "description": "Array or comma-separated list. Examples: [\"query\"], [\"query\",\"page\"]. Also supports \"hour\" when used with hourly_all.", "anyOf": [{"type":"array","items":{"type":"string"}},{"type":"string"}] },
                            "row_limit": { "type": "integer", "minimum": 1, "maximum": 25000, "default": 1000 },
                            "start_row": { "type": "integer", "minimum": 0, "default": 0 },
                            "aggregation_type": { "type": "string", "enum": ["auto", "byProperty", "byPage", "byNewsShowcasePanel"], "default": "auto" },
                            "type": { "type": "string", "description": "Search type: web|image|video|news|discover|googleNews" },
                            "data_state": { "type": "string", "enum": ["final", "all", "hourly_all"], "default": "final", "description": "Use hourly_all for hourly data (limited lookback window)." },
                            "dimension_filter_groups": { "description": "Optional raw JSON (array/object). See Search Console API dimensionFilterGroups schema.", "type": ["array", "object", "string"] },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url", "start_date", "end_date"],
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
                name: Cow::Borrowed("list_sitemaps"),
                title: None,
                description: Some(Cow::Borrowed("List sitemaps for a property.")),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Property URL, e.g. https://example.com/ or sc-domain:example.com" },
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
                name: Cow::Borrowed("get_sitemap"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get details for a specific sitemap (errors, warnings, last submitted, etc.).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Property URL, e.g. https://example.com/ or sc-domain:example.com" },
                            "feedpath": { "type": "string", "description": "Sitemap URL (feedpath in the Search Console API)." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url", "feedpath"],
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
                name: Cow::Borrowed("submit_sitemap"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Submit a sitemap for a property (PUT sitemap).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Property URL, e.g. https://example.com/ or sc-domain:example.com" },
                            "feedpath": { "type": "string", "description": "Sitemap URL (feedpath in the Search Console API)." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url", "feedpath"],
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
                name: Cow::Borrowed("delete_sitemap"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Delete a sitemap for a property (DELETE sitemap).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Property URL, e.g. https://example.com/ or sc-domain:example.com" },
                            "feedpath": { "type": "string", "description": "Sitemap URL (feedpath in the Search Console API)." },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url", "feedpath"],
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
                name: Cow::Borrowed("inspect_url"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Inspect a URL for indexing status (URL Inspection API).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "site_url": { "type": "string", "description": "Property URL or sc-domain:..." },
                            "inspection_url": { "type": "string", "description": "The URL to inspect." },
                            "language_code": { "type": "string", "description": "IETF language code, e.g. en-US" },
                            "response_format": { "type": "string", "enum": ["concise", "detailed"], "default": "concise" }
                        },
                        "required": ["site_url", "inspection_url"],
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
                name: Cow::Borrowed("query_builder"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Helper to build common Search Analytics queries (returns args for `search_analytics`).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query_type": {
                                "type": "string",
                                "enum": [
                                    "top_queries",
                                    "top_pages",
                                    "query_performance_over_time",
                                    "page_performance_over_time",
                                    "country_breakdown",
                                    "device_breakdown",
                                    "queries_for_page",
                                    "pages_for_query",
                                    "low_ctr_opportunities",
                                    "position_changes"
                                ]
                            },
                            "site_url": { "type": "string", "description": "Property URL, e.g. https://example.com/ or sc-domain:example.com" },
                            "days": { "type": "integer", "minimum": 1, "maximum": 365, "default": 28, "description": "Lookback window ending today (UTC)." },
                            "filter": { "type": "string", "description": "Optional filter value (e.g. a page URL for queries_for_page, or a query for pages_for_query)." }
                        },
                        "required": ["query_type", "site_url"],
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
        match request.name.as_ref() {
            "list_sites" => {
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );
                let v = self.gsc_get(&format!("{}/sites", GSC_API_BASE)).await?;
                if !concise {
                    return structured_result_with_text(&v, None);
                }
                let entries = v
                    .get("siteEntry")
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default();
                let simplified: Vec<Value> = entries
                    .into_iter()
                    .map(|e| {
                        json!({
                            "site_url": e.get("siteUrl").cloned().unwrap_or(Value::Null),
                            "permission_level": e.get("permissionLevel").cloned().unwrap_or(Value::Null)
                        })
                    })
                    .collect();
                structured_result_with_text(&json!({ "sites": simplified }), None)
            }
            "get_site" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );
                let encoded = urlencoding::encode(site_url);
                let url = format!("{}/sites/{}", GSC_API_BASE, encoded);
                let resp = self.gsc_get(&url).await?;
                if !concise {
                    return structured_result_with_text(&resp, None);
                }
                structured_result_with_text(
                    &json!({
                        "site_url": resp.get("siteUrl").cloned().unwrap_or_else(|| json!(site_url)),
                        "permission_level": resp.get("permissionLevel").cloned().unwrap_or(Value::Null)
                    }),
                    None,
                )
            }
            "search_analytics" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let start_date = args.get("start_date").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("start_date is required".to_string()),
                )?;
                let end_date = args.get("end_date").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("end_date is required".to_string()),
                )?;

                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let mut body = Map::new();
                body.insert("startDate".to_string(), json!(start_date));
                body.insert("endDate".to_string(), json!(end_date));

                if let Some(dims) = args.get("dimensions") {
                    let parsed: Vec<String> = match dims {
                        Value::Array(arr) => arr
                            .iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect(),
                        Value::String(s) => s
                            .split(',')
                            .map(|x| x.trim())
                            .filter(|x| !x.is_empty())
                            .map(|x| x.to_string())
                            .collect(),
                        _ => Vec::new(),
                    };
                    if !parsed.is_empty() {
                        body.insert("dimensions".to_string(), json!(parsed));
                    }
                }

                let row_limit = args
                    .get("row_limit")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1000)
                    .clamp(1, 25_000);
                body.insert("rowLimit".to_string(), json!(row_limit));
                let start_row = args.get("start_row").and_then(|v| v.as_i64()).unwrap_or(0);
                body.insert("startRow".to_string(), json!(start_row.max(0)));

                if let Some(agg) = args.get("aggregation_type").and_then(|v| v.as_str()) {
                    body.insert("aggregationType".to_string(), json!(agg));
                }
                if let Some(t) = args.get("type").and_then(|v| v.as_str()) {
                    body.insert("type".to_string(), json!(t));
                }
                if let Some(ds) = args.get("data_state").and_then(|v| v.as_str()) {
                    body.insert("dataState".to_string(), json!(ds));
                }
                if let Some(filters) = args.get("dimension_filter_groups") {
                    let parsed = match filters {
                        Value::String(s) => {
                            let st = s.trim();
                            if st.starts_with('{') || st.starts_with('[') {
                                serde_json::from_str::<Value>(st).unwrap_or_else(|_| json!(s))
                            } else {
                                json!(s)
                            }
                        }
                        other => other.clone(),
                    };
                    if !parsed.is_null() {
                        body.insert("dimensionFilterGroups".to_string(), parsed);
                    }
                }

                let encoded = urlencoding::encode(site_url);
                let url = format!("{}/sites/{}/searchAnalytics/query", GSC_API_BASE, encoded);
                let resp = self.gsc_post(&url, &Value::Object(body.clone())).await?;

                if !concise {
                    return structured_result_with_text(
                        &json!({ "request": Value::Object(body), "response": resp }),
                        None,
                    );
                }
                let rows = resp.get("rows").cloned().unwrap_or_else(|| json!([]));
                let response_agg = resp
                    .get("responseAggregationType")
                    .cloned()
                    .unwrap_or(Value::Null);
                structured_result_with_text(
                    &json!({
                        "site_url": site_url,
                        "start_date": start_date,
                        "end_date": end_date,
                        "response_aggregation_type": response_agg,
                        "rows": rows
                    }),
                    None,
                )
            }
            "list_sitemaps" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );
                let encoded = urlencoding::encode(site_url);
                let url = format!("{}/sites/{}/sitemaps", GSC_API_BASE, encoded);
                let resp = self.gsc_get(&url).await?;
                if !concise {
                    return structured_result_with_text(&resp, None);
                }
                let sitemaps = resp.get("sitemap").cloned().unwrap_or_else(|| json!([]));
                structured_result_with_text(&json!({ "sitemaps": sitemaps }), None)
            }
            "get_sitemap" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let feedpath = args.get("feedpath").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("feedpath is required".to_string()),
                )?;
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let encoded_site = urlencoding::encode(site_url);
                let encoded_feed = urlencoding::encode(feedpath);
                let url = format!(
                    "{}/sites/{}/sitemaps/{}",
                    GSC_API_BASE, encoded_site, encoded_feed
                );
                let resp = self.gsc_get(&url).await?;
                if !concise {
                    return structured_result_with_text(&resp, None);
                }
                structured_result_with_text(
                    &json!({
                        "site_url": site_url,
                        "feedpath": feedpath,
                        "sitemap": resp
                    }),
                    None,
                )
            }
            "submit_sitemap" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let feedpath = args.get("feedpath").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("feedpath is required".to_string()),
                )?;
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let encoded_site = urlencoding::encode(site_url);
                let encoded_feed = urlencoding::encode(feedpath);
                let url = format!(
                    "{}/sites/{}/sitemaps/{}",
                    GSC_API_BASE, encoded_site, encoded_feed
                );
                let resp = self.gsc_put(&url).await?;
                if !concise {
                    return structured_result_with_text(&resp, None);
                }
                structured_result_with_text(
                    &json!({ "site_url": site_url, "feedpath": feedpath, "result": resp }),
                    None,
                )
            }
            "delete_sitemap" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let feedpath = args.get("feedpath").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("feedpath is required".to_string()),
                )?;
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let encoded_site = urlencoding::encode(site_url);
                let encoded_feed = urlencoding::encode(feedpath);
                let url = format!(
                    "{}/sites/{}/sitemaps/{}",
                    GSC_API_BASE, encoded_site, encoded_feed
                );
                let resp = self.gsc_delete(&url).await?;
                if !concise {
                    return structured_result_with_text(&resp, None);
                }
                structured_result_with_text(
                    &json!({ "site_url": site_url, "feedpath": feedpath, "result": resp }),
                    None,
                )
            }
            "inspect_url" => {
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let inspection_url = args.get("inspection_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("inspection_url is required".to_string()),
                )?;
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let mut body = Map::new();
                body.insert("inspectionUrl".to_string(), json!(inspection_url));
                body.insert("siteUrl".to_string(), json!(site_url));
                if let Some(lang) = args.get("language_code").and_then(|v| v.as_str()) {
                    body.insert("languageCode".to_string(), json!(lang));
                }
                let resp = self
                    .gsc_post(GSC_URL_INSPECTION_ENDPOINT, &Value::Object(body))
                    .await?;

                if !concise {
                    return structured_result_with_text(&resp, None);
                }
                let inspection_result = resp
                    .get("inspectionResult")
                    .cloned()
                    .unwrap_or_else(|| resp.clone());
                structured_result_with_text(&inspection_result, None)
            }
            "query_builder" => {
                let query_type =
                    args.get("query_type")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("query_type is required".to_string())
                        })?;
                let site_url = args.get("site_url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("site_url is required".to_string()),
                )?;
                let days = args
                    .get("days")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(28)
                    .clamp(1, 365);
                let filter = args.get("filter").and_then(|v| v.as_str());

                let end = chrono::Utc::now().date_naive();
                let start = end - chrono::Duration::days(days);
                let start_date = start.format("%Y-%m-%d").to_string();
                let end_date = end.format("%Y-%m-%d").to_string();

                let mut out = Map::new();
                out.insert("site_url".to_string(), json!(site_url));
                out.insert("start_date".to_string(), json!(start_date));
                out.insert("end_date".to_string(), json!(end_date));
                out.insert("row_limit".to_string(), json!(1000));
                out.insert("start_row".to_string(), json!(0));
                out.insert("aggregation_type".to_string(), json!("auto"));
                out.insert("data_state".to_string(), json!("final"));

                match query_type {
                    "top_queries" => {
                        out.insert("dimensions".to_string(), json!(["query"]));
                    }
                    "top_pages" => {
                        out.insert("dimensions".to_string(), json!(["page"]));
                    }
                    "query_performance_over_time" => {
                        out.insert("dimensions".to_string(), json!(["date", "query"]));
                    }
                    "page_performance_over_time" => {
                        out.insert("dimensions".to_string(), json!(["date", "page"]));
                    }
                    "country_breakdown" => {
                        out.insert("dimensions".to_string(), json!(["country"]));
                    }
                    "device_breakdown" => {
                        out.insert("dimensions".to_string(), json!(["device"]));
                    }
                    "queries_for_page" => {
                        out.insert("dimensions".to_string(), json!(["query"]));
                        if let Some(page_url) = filter {
                            out.insert(
                                "dimension_filter_groups".to_string(),
                                json!([{
                                    "filters": [{
                                        "dimension": "page",
                                        "operator": "equals",
                                        "expression": page_url
                                    }]
                                }]),
                            );
                        }
                    }
                    "pages_for_query" => {
                        out.insert("dimensions".to_string(), json!(["page"]));
                        if let Some(q) = filter {
                            out.insert(
                                "dimension_filter_groups".to_string(),
                                json!([{
                                    "filters": [{
                                        "dimension": "query",
                                        "operator": "equals",
                                        "expression": q
                                    }]
                                }]),
                            );
                        }
                    }
                    "low_ctr_opportunities" => {
                        out.insert("dimensions".to_string(), json!(["query"]));
                    }
                    "position_changes" => {
                        out.insert("dimensions".to_string(), json!(["query"]));
                    }
                    _ => {
                        return Err(ConnectorError::InvalidParams(format!(
                            "Unknown query_type: {}",
                            query_type
                        )));
                    }
                }

                structured_result_with_text(&Value::Object(out), None)
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
        let auth = self.merged_auth().await;
        if auth.contains_key("access_token") || auth.contains_key("refresh_token") {
            return Ok(());
        }
        Err(ConnectorError::Authentication(
            "Google Search Console auth not configured".to_string(),
        ))
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "access_token".to_string(),
                    label: "Access Token".to_string(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some("Prefer `rzn-tools setup google-search-console` to configure OAuth tokens automatically.".to_string()),
                    options: None,
                },
                Field {
                    name: "refresh_token".to_string(),
                    label: "Refresh Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("Optional but recommended for long-lived usage.".to_string()),
                    options: None,
                },
                Field {
                    name: "client_id".to_string(),
                    label: "Client ID".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("OAuth client ID (needed for refresh_token flows).".to_string()),
                    options: None,
                },
                Field {
                    name: "client_secret".to_string(),
                    label: "Client Secret".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("OAuth client secret (optional for some installed app flows).".to_string()),
                    options: None,
                },
                Field {
                    name: "expires_at".to_string(),
                    label: "Expires At".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Epoch seconds when access_token expires (managed by setup/refresh).".to_string()),
                    options: None,
                },
            ],
        }
    }
}
