use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use reqwest::{Client, Method, StatusCode};
use rmcp::model::*;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::{Connector, URLParamExtraction, URLPatternSpec};

const ITUNES_BASE_URL: &str = "https://itunes.apple.com";
const APP_STORE_USER_AGENT: &str = "rzn-tools/app-store";

#[derive(Debug, Deserialize)]
struct SearchInput {
    query: String,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct LookupInput {
    track_id: u64,
    #[serde(default)]
    country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReviewsInput {
    track_id: u64,
}

pub struct AppStoreConnector {
    http: Client,
}

impl AppStoreConnector {
    pub async fn new(_auth: AuthDetails) -> Result<Self, ConnectorError> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(APP_STORE_USER_AGENT));
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        let http = Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(ConnectorError::HttpRequest)?;
        Ok(Self { http })
    }

    async fn request_json(
        &self,
        method: Method,
        path: &str,
        query: Vec<(String, String)>,
    ) -> Result<Value, ConnectorError> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{ITUNES_BASE_URL}{}", path)
        };

        let mut req = self.http.request(method, &url);
        if !query.is_empty() {
            req = req.query(&query);
        }

        let resp = req.send().await.map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Err(ConnectorError::ResourceNotFound);
        }
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "iTunes API returned HTTP {}",
                status
            )));
        }
        resp.json::<Value>()
            .await
            .map_err(ConnectorError::HttpRequest)
    }
}

#[async_trait]
impl Connector for AppStoreConnector {
    fn name(&self) -> &'static str {
        "app-store"
    }

    fn description(&self) -> &'static str {
        "Public App Store app metadata via the iTunes Search API (search, lookup, reviews)."
    }

    fn display_name(&self) -> &'static str {
        "App Store"
    }

    fn icon(&self) -> &'static str {
        "app_store"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["app_store", "metadata", "mobile"]
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: r"(?:https?://)?apps\.apple\.com/[^/]+/app/[^/]+/id(\d+)(?:[/?#].*)?"
                .to_string(),
            default_tool: "lookup".to_string(),
            description: "Fetch App Store app details by apps.apple.com URL".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 1,
                param_name: "track_id".to_string(),
                use_full_url: false,
            }],
        }]
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
                "Use `search` to discover apps (iTunes Search API), then `lookup` or `reviews` \
by `track_id` (aka App Store / adam id)."
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
                name: Cow::Borrowed("search"),
                title: None,
                description: Some(Cow::Borrowed("Search apps by keyword (iTunes Search API).")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "query":{"type":"string"},
                            "country":{"type":"string","description":"ISO 2-letter storefront code, e.g. US (default US)"},
                            "limit":{"type":"integer","minimum":1,"maximum":200,"default":25}
                        },
                        "required":["query"]
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("lookup"),
                title: None,
                description: Some(Cow::Borrowed("Lookup app details by App Store track_id (adam id).")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "track_id":{"type":"integer"},
                            "country":{"type":"string","description":"ISO 2-letter storefront code, e.g. US (default US)"}
                        },
                        "required":["track_id"]
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("reviews"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch recent customer reviews via the public App Store RSS feed (JSON).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "track_id":{"type":"integer"}
                        },
                        "required":["track_id"]
                    })
                    .as_object()
                    .expect("schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("test_auth"),
                title: None,
                description: Some(Cow::Borrowed("Smoke test iTunes Search API connectivity.")),
                input_schema: Arc::new(
                    json!({"type":"object","properties":{}})
                        .as_object()
                        .expect("schema object")
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
        let args_map: Map<String, Value> = request.arguments.unwrap_or_default();

        match request.name.as_ref() {
            "search" => {
                let input: SearchInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let country = input.country.unwrap_or_else(|| "US".to_string());
                let limit = input.limit.unwrap_or(25).clamp(1, 200);
                let v = self
                    .request_json(
                        Method::GET,
                        "/search",
                        vec![
                            ("term".into(), input.query),
                            ("entity".into(), "software".into()),
                            ("country".into(), country),
                            ("limit".into(), limit.to_string()),
                        ],
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "lookup" => {
                let input: LookupInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let country = input.country.unwrap_or_else(|| "US".to_string());
                let v = self
                    .request_json(
                        Method::GET,
                        "/lookup",
                        vec![
                            ("id".into(), input.track_id.to_string()),
                            ("country".into(), country),
                        ],
                    )
                    .await?;
                structured_result_with_text(&v, None)
            }
            "reviews" => {
                let input: ReviewsInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let path = format!(
                    "/rss/customerreviews/id={}/sortBy=mostRecent/json",
                    input.track_id
                );
                let v = self.request_json(Method::GET, &path, Vec::new()).await?;
                structured_result_with_text(&v, None)
            }
            "test_auth" => {
                self.test_auth().await?;
                structured_result_with_text(&json!({"ok": true}), None)
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
        Ok(AuthDetails::new())
    }

    async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let _ = self
            .request_json(
                Method::GET,
                "/search",
                vec![
                    ("term".into(), "test".into()),
                    ("entity".into(), "software".into()),
                    ("country".into(), "US".into()),
                    ("limit".into(), "1".into()),
                ],
            )
            .await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: vec![] }
    }
}
