use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use reqwest::Client;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

pub struct ParallelSearchConnector {
    client: Client,
    api_key: Option<String>,
}

impl ParallelSearchConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.1.0")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        let api_key = auth
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("PARALLEL_API_KEY").ok());
        Ok(Self { client, api_key })
    }

    fn get_headers(&self) -> Result<HeaderMap, ConnectorError> {
        let key = self.api_key.as_ref().ok_or_else(|| {
            ConnectorError::InvalidInput(
                "Missing credentials: set PARALLEL_API_KEY or use rzn-tools setup parallel-search"
                    .into(),
            )
        })?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(key).map_err(|e| {
                ConnectorError::InvalidInput(format!("Invalid API Key header: {}", e))
            })?,
        );
        Ok(headers)
    }

    async fn create_monitor_impl(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'query'".into()))?;

        let cadence = args
            .get("cadence")
            .and_then(|v| v.as_str())
            .unwrap_or("daily");

        let mut body = json!({
            "query": query,
            "cadence": cadence
        });

        // Webhook configuration
        if let Some(webhook_url) = args.get("webhook_url").and_then(|v| v.as_str()) {
            body["webhook"] = json!({
                "url": webhook_url,
                "event_types": ["monitor.event.detected", "monitor.execution.completed"]
            });
        }

        // Output schema for structured events
        if let Some(schema) = args.get("output_schema") {
            body["output_schema"] = schema.clone();
        }

        // Metadata
        if let Some(metadata) = args.get("metadata") {
            body["metadata"] = metadata.clone();
        }

        let headers = self.get_headers()?;
        let resp = self
            .client
            .post("https://api.parallel.ai/v1/monitors")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Parallel API error: {} - {}",
                status, value
            )));
        }

        let data = json!({
            "provider": "parallel-ai",
            "operation": "create_monitor",
            "monitor": value
        });

        structured_result_with_text(&data, None)
    }

    async fn list_monitors_impl(&self) -> Result<CallToolResult, ConnectorError> {
        let headers = self.get_headers()?;
        let resp = self
            .client
            .get("https://api.parallel.ai/v1/monitors")
            .headers(headers)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Parallel API error: {} - {}",
                status, value
            )));
        }

        let data = json!({
            "provider": "parallel-ai",
            "operation": "list_monitors",
            "monitors": value.get("monitors").cloned().unwrap_or_else(|| json!([]))
        });

        structured_result_with_text(&data, None)
    }

    async fn get_monitor_events_impl(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let monitor_id = args
            .get("monitor_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'monitor_id'".into()))?;

        let headers = self.get_headers()?;
        let resp = self
            .client
            .get(format!(
                "https://api.parallel.ai/v1/monitors/{}/events",
                monitor_id
            ))
            .headers(headers)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Parallel API error: {} - {}",
                status, value
            )));
        }

        let data = json!({
            "provider": "parallel-ai",
            "operation": "get_monitor_events",
            "monitor_id": monitor_id,
            "events": value.get("events").cloned().unwrap_or_else(|| json!([]))
        });

        structured_result_with_text(&data, None)
    }

    async fn cancel_monitor_impl(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let monitor_id = args
            .get("monitor_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'monitor_id'".into()))?;

        let headers = self.get_headers()?;
        let resp = self
            .client
            .post(format!(
                "https://api.parallel.ai/v1/monitors/{}/cancel",
                monitor_id
            ))
            .headers(headers)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Parallel API error: {} - {}",
                status, value
            )));
        }

        let data = json!({
            "provider": "parallel-ai",
            "operation": "cancel_monitor",
            "monitor_id": monitor_id,
            "status": "canceled"
        });

        structured_result_with_text(&data, None)
    }
}

#[async_trait]
impl Connector for ParallelSearchConnector {
    fn name(&self) -> &'static str {
        "parallel-search"
    }
    fn description(&self) -> &'static str {
        "Broad web search, multi-query fan-out, and recurring monitors via Parallel AI."
    }

    fn display_name(&self) -> &'static str {
        "Parallel Search"
    }

    fn icon(&self) -> &'static str {
        "parallel"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["search", "web", "ai"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
    }

    async fn initialize(
        &self,
        _r: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().into(),
                version: "0.2.0".into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
r#"Parallel Search is for broad web objectives, query fan-out, and ongoing monitoring.

Use Parallel Search when you need:
- one natural-language search objective answered from the open web
- multiple parallel subqueries for comparison or coverage
- recurring monitors that detect new web events on a cadence
- token-efficient repeated search inside an agent loop

Preferred tool flow:
1. search -> broad discovery, comparisons, or decomposed subqueries with search_queries.
2. create_monitor -> persistent tracking for announcements, pricing, funding, or policy changes.
3. list_monitors -> inspect current monitors before reading or canceling.
4. get_monitor_events -> fetch detections from a monitor.
5. cancel_monitor -> stop a monitor that is no longer needed.

Use Exa instead when the task is entity-typed lookup (people, companies, papers, repos, tweets, filings), seed-based similarity, or a single grounded cited answer."#.into(),
            ),
        })
    }
    async fn list_resources(
        &self,
        _r: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        })
    }
    async fn read_resource(
        &self,
        _r: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Ok(vec![])
    }

    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let search_tool = Tool {
            name: Cow::Borrowed("search"),
            title: Some("Parallel Web Search".into()),
            description: Some(Cow::Borrowed(
                "Search the open web from one natural-language objective, with optional fan-out subqueries. Best for broad questions, comparisons, recent developments, and agent loops. Key args: query (required objective); optional search_queries for decomposition, mode='agentic' for loops, limit, after_date, include_domains/exclude_domains, max_age_seconds.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Natural language objective describing what you're looking for. Example: 'Find the latest AI agent frameworks and their key features'"
                    },
                    "search_queries": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Optional keyword queries to run in parallel. Example: ['AI agent frameworks 2024', 'LangChain vs AutoGPT', 'CrewAI features']"
                    },
                    "mode": {
                        "type": "string",
                        "enum": ["one-shot", "agentic"],
                        "description": "one-shot=comprehensive results (default), agentic=token-efficient for agent loops"
                    },
                    "limit": {
                        "type": "integer",
                        "default": 10,
                        "description": "Maximum results to return"
                    },
                    "max_results": {"type": "integer", "description": "Alias for limit (deprecated)."},
                    "after_date": {
                        "type": "string",
                        "description": "Only include content published after this date. Format: YYYY-MM-DD. Example: '2024-01-01'"
                    },
                    "include_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Restrict to these domains. Example: ['techcrunch.com', 'wired.com']"
                    },
                    "exclude_domains": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Exclude these domains from results"
                    },
                    "max_age_seconds": {
                        "type": "integer",
                        "description": "Max cache age in seconds. Set low (e.g., 3600) for fresh content. Default uses cached results."
                    }
                },
                "required": ["query"]
            }).as_object().expect("Schema object").clone()),
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
            icons: None,
        };

        // Monitor tools
        let create_monitor_tool = Tool {
            name: Cow::Borrowed("create_monitor"),
            title: Some("Create Monitor".into()),
            description: Some(Cow::Borrowed(
                "Create a recurring web monitor that reruns a search objective on a schedule and can emit webhook events. Use for ongoing tracking such as announcements, pricing changes, funding news, or regulatory updates. Key args: query (required objective to monitor); optional cadence, webhook_url, output_schema, metadata.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query to monitor. Example: 'OpenAI announcements' or 'Tesla price changes'"
                    },
                    "cadence": {
                        "type": "string",
                        "enum": ["hourly", "daily", "weekly"],
                        "description": "How often to check. hourly=every hour, daily=once/day (default), weekly=once/week"
                    },
                    "webhook_url": {
                        "type": "string",
                        "description": "URL to receive webhook notifications when new content is detected"
                    },
                    "output_schema": {
                        "type": "object",
                        "description": "JSON Schema for structured event extraction"
                    },
                    "metadata": {
                        "type": "object",
                        "description": "Custom metadata to include with events (e.g., slack_channel_id)"
                    }
                },
                "required": ["query"]
            }).as_object().expect("Schema object").clone()),
            output_schema: None,
            annotations: Some(
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(false)
                    .idempotent(false)
                    .open_world(true),
            ),
            icons: None,
        };

        let list_monitors_tool = Tool {
            name: Cow::Borrowed("list_monitors"),
            title: Some("List Monitors".into()),
            description: Some(Cow::Borrowed(
                "List active monitors so an agent can inspect existing tracking state before reading events or creating duplicates. Key args: none.",
            )),
            input_schema: Arc::new(
                json!({
                    "type": "object",
                    "properties": {}
                })
                .as_object()
                .expect("Schema object")
                .clone(),
            ),
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
            icons: None,
        };

        let get_monitor_events_tool = Tool {
            name: Cow::Borrowed("get_monitor_events"),
            title: Some("Get Monitor Events".into()),
            description: Some(Cow::Borrowed(
                "Fetch detected events for one monitor created earlier. Use after create_monitor or list_monitors when you want the actual matched updates. Key args: monitor_id (required).",
            )),
            input_schema: Arc::new(
                json!({
                    "type": "object",
                    "properties": {
                        "monitor_id": {
                            "type": "string",
                            "description": "The monitor ID (from create_monitor or list_monitors)"
                        }
                    },
                    "required": ["monitor_id"]
                })
                .as_object()
                .expect("Schema object")
                .clone(),
            ),
            output_schema: None,
            annotations: Some(ToolAnnotations::new().read_only(true).open_world(true)),
            icons: None,
        };

        let cancel_monitor_tool = Tool {
            name: Cow::Borrowed("cancel_monitor"),
            title: Some("Cancel Monitor".into()),
            description: Some(Cow::Borrowed(
                "Stop an active monitor that is no longer needed. This changes remote state, so use it deliberately. Key args: monitor_id (required).",
            )),
            input_schema: Arc::new(
                json!({
                    "type": "object",
                    "properties": {
                        "monitor_id": {
                            "type": "string",
                            "description": "The monitor ID to cancel"
                        }
                    },
                    "required": ["monitor_id"]
                })
                .as_object()
                .expect("Schema object")
                .clone(),
            ),
            output_schema: None,
            annotations: Some(
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(true)
                    .idempotent(true)
                    .open_world(true),
            ),
            icons: None,
        };

        Ok(ListToolsResult {
            tools: vec![
                search_tool,
                create_monitor_tool,
                list_monitors_tool,
                get_monitor_events_tool,
                cancel_monitor_tool,
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
            "create_monitor" => return self.create_monitor_impl(&args).await,
            "list_monitors" => return self.list_monitors_impl().await,
            "get_monitor_events" => return self.get_monitor_events_impl(&args).await,
            "cancel_monitor" => return self.cancel_monitor_impl(&args).await,
            "search" => {}
            _ => return Err(ConnectorError::ToolNotFound),
        }

        // Search implementation
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'query'".into()))?;

        let search_queries: Vec<String> = if let Some(queries_val) = args.get("search_queries") {
            if let Some(queries_array) = queries_val.as_array() {
                queries_array
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            } else {
                vec![query.to_string()]
            }
        } else {
            vec![query.to_string()]
        };

        let max_results = args
            .get("limit")
            .or_else(|| args.get("max_results"))
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;

        let include_domains = args
            .get("include_domains")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(|x| x.to_string()))
                    .collect::<Vec<_>>()
            });
        let exclude_domains = args
            .get("exclude_domains")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(|x| x.to_string()))
                    .collect::<Vec<_>>()
            });

        let headers = self.get_headers()?;

        let mut body = json!({
            "objective": query,
            "search_queries": search_queries,
            "max_results": max_results,
            "excerpts": {
                "max_chars_per_result": 10000
            }
        });

        // Mode: one-shot (comprehensive) vs agentic (token-efficient for loops)
        if let Some(mode) = args.get("mode").and_then(|v| v.as_str()) {
            body["mode"] = json!(mode);
        }

        // Source policy
        let mut source_policy = json!({});
        if let Some(v) = include_domains {
            source_policy["include_domains"] = json!(v);
        }
        if let Some(v) = exclude_domains {
            source_policy["exclude_domains"] = json!(v);
        }
        if let Some(after_date) = args.get("after_date").and_then(|v| v.as_str()) {
            source_policy["after_date"] = json!(after_date);
        }
        if !source_policy.as_object().unwrap().is_empty() {
            body["source_policy"] = source_policy;
        }

        // Fetch policy for cache control
        if let Some(max_age) = args.get("max_age_seconds").and_then(|v| v.as_u64()) {
            body["fetch_policy"] = json!({
                "max_age_seconds": max_age
            });
        }

        let resp = self
            .client
            .post("https://api.parallel.ai/v1beta/search")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Parallel AI API error: {} - {}",
                status, value
            )));
        }

        let data = json!({
            "provider": "parallel-ai",
            "objective": query,
            "search_queries": search_queries,
            "max_results": max_results,
            "results": value.get("results").cloned().unwrap_or_else(|| json!([]))
        });

        structured_result_with_text(&data, None)
    }

    async fn list_prompts(
        &self,
        _r: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }
    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::ToolNotFound)
    }
    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        let mut a = AuthDetails::new();
        if let Some(v) = &self.api_key {
            a.insert("api_key".into(), v.clone());
        }
        Ok(a)
    }
    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.api_key = details
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("PARALLEL_API_KEY").ok());
        Ok(())
    }
    async fn test_auth(&self) -> Result<(), ConnectorError> {
        if self
            .api_key
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
        {
            // For a more robust test, one might make a dummy API call, but for simplicity,
            // just checking for API key presence is often sufficient for initial auth test.
            Ok(())
        } else {
            Err(ConnectorError::InvalidInput("Missing api_key".into()))
        }
    }
    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![Field {
                name: "api_key".into(),
                label: "Parallel AI API Key".into(),
                field_type: FieldType::Secret,
                required: true,
                description: Some(
                    "Set PARALLEL_API_KEY env var or configure via `rzn-tools config set parallel-search --value <key>`."
                        .into(),
                ),
                options: None,
            }],
        }
    }
}
