use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{build_filters_clause, resolve_search_filters, structured_result_with_text};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct XSearchOptions {
    allowed_x_handles: Vec<String>,
    excluded_x_handles: Vec<String>,
    from_date: Option<String>,
    to_date: Option<String>,
    enable_image_understanding: bool,
    enable_video_understanding: bool,
}

#[derive(Clone)]
pub struct XaiSearchConnector {
    client: Client,
    api_key: Option<String>,
    default_model: String,
}

impl XaiSearchConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.1.0")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        let api_key = auth
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("XAI_API_KEY").ok());
        let default_model = auth
            .get("model")
            .cloned()
            .unwrap_or_else(|| "grok-4-fast".to_string());

        Ok(Self {
            client,
            api_key,
            default_model,
        })
    }

    fn collect_string_array(args: &serde_json::Map<String, Value>, key: &str) -> Vec<String> {
        args.get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(|x| x.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn resolve_x_search_options(
        args: &serde_json::Map<String, Value>,
        filters: &crate::utils::SearchFilters,
    ) -> Result<XSearchOptions, ConnectorError> {
        let allowed_x_handles = Self::collect_string_array(args, "allowed_x_handles");
        let excluded_x_handles = Self::collect_string_array(args, "excluded_x_handles");
        if !allowed_x_handles.is_empty() && !excluded_x_handles.is_empty() {
            return Err(ConnectorError::InvalidParams(
                "x_search accepts either allowed_x_handles or excluded_x_handles, not both".into(),
            ));
        }
        if allowed_x_handles.len() > 10 {
            return Err(ConnectorError::InvalidParams(
                "allowed_x_handles supports at most 10 handles".into(),
            ));
        }
        if excluded_x_handles.len() > 10 {
            return Err(ConnectorError::InvalidParams(
                "excluded_x_handles supports at most 10 handles".into(),
            ));
        }

        Ok(XSearchOptions {
            allowed_x_handles,
            excluded_x_handles,
            from_date: args
                .get("from_date")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| filters.since.clone()),
            to_date: args
                .get("to_date")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| filters.until.clone()),
            enable_image_understanding: args
                .get("enable_image_understanding")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            enable_video_understanding: args
                .get("enable_video_understanding")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }

    fn build_x_search_prompt_clause(
        args: &serde_json::Map<String, Value>,
        filters: &crate::utils::SearchFilters,
        options: &XSearchOptions,
    ) -> String {
        let mut clause = build_filters_clause(filters);
        let mut parts = Vec::new();

        if args.contains_key("from_date") || args.contains_key("to_date") {
            if let Some(v) = &options.from_date {
                parts.push(format!("from_date={}", v));
            }
            if let Some(v) = &options.to_date {
                parts.push(format!("to_date={}", v));
            }
        }
        if !options.allowed_x_handles.is_empty() {
            parts.push(format!("allowed_x_handles={:?}", options.allowed_x_handles));
        }
        if !options.excluded_x_handles.is_empty() {
            parts.push(format!(
                "excluded_x_handles={:?}",
                options.excluded_x_handles
            ));
        }
        if options.enable_image_understanding {
            parts.push("enable_image_understanding=true".to_string());
        }
        if options.enable_video_understanding {
            parts.push("enable_video_understanding=true".to_string());
        }

        if !parts.is_empty() {
            if clause.is_empty() {
                clause = format!("\nX Search filters: {}", parts.join("; "));
            } else {
                clause.push_str(&format!("\nX Search filters: {}", parts.join("; ")));
            }
        }

        clause
    }

    fn build_tools(
        sources: &[String],
        mode: &str,
        limit: usize,
        filters: &crate::utils::SearchFilters,
        x_options: &XSearchOptions,
    ) -> Vec<Value> {
        let mut tools = Vec::new();
        let search_mode = match mode {
            "on" | "off" | "auto" => mode,
            _ => "auto",
        };

        let has_web = sources.iter().any(|s| s == "web");
        if has_web {
            let mut web_tool = json!({
                "type": "web_search",
                "search_mode": search_mode,
                "max_search_results": limit,
            });

            let mut web_filters = serde_json::Map::new();
            if !filters.include_domains.is_empty() {
                web_filters.insert(
                    "allowed_websites".to_string(),
                    json!(filters.include_domains.clone()),
                );
            }
            if !filters.exclude_domains.is_empty() {
                web_filters.insert(
                    "blocked_websites".to_string(),
                    json!(filters.exclude_domains.clone()),
                );
            }
            if !web_filters.is_empty() {
                web_tool["filters"] = Value::Object(web_filters);
            }
            if x_options.enable_image_understanding {
                web_tool["enable_image_understanding"] = json!(true);
            }

            tools.push(web_tool);
        }

        let has_x = sources.iter().any(|s| s == "x");
        if has_x {
            let mut x_tool = json!({
                "type": "x_search",
                "search_mode": search_mode,
                "max_search_results": limit,
            });
            if let Some(from_date) = &x_options.from_date {
                x_tool["from_date"] = json!(from_date);
            }
            if let Some(to_date) = &x_options.to_date {
                x_tool["to_date"] = json!(to_date);
            }
            if !x_options.allowed_x_handles.is_empty() {
                x_tool["allowed_x_handles"] = json!(x_options.allowed_x_handles);
            }
            if !x_options.excluded_x_handles.is_empty() {
                x_tool["excluded_x_handles"] = json!(x_options.excluded_x_handles);
            }
            if x_options.enable_image_understanding {
                x_tool["enable_image_understanding"] = json!(true);
            }
            if x_options.enable_video_understanding {
                x_tool["enable_video_understanding"] = json!(true);
            }
            tools.push(x_tool);
        }

        // Safety fallback when the caller passed no valid sources.
        if tools.is_empty() {
            tools.push(json!({
                "type": "web_search",
                "search_mode": search_mode,
                "max_search_results": limit
            }));
        }
        tools
    }

    fn extract_output_text(value: &Value) -> String {
        if let Some(text) = value.get("output_text").and_then(Value::as_str) {
            return text.to_string();
        }

        let mut parts = Vec::new();
        if let Some(outputs) = value.get("output").and_then(Value::as_array) {
            for item in outputs {
                if item.get("type").and_then(Value::as_str) != Some("message") {
                    continue;
                }
                if let Some(content) = item.get("content").and_then(Value::as_array) {
                    for c in content {
                        if c.get("type").and_then(Value::as_str) == Some("output_text") {
                            if let Some(text) = c.get("text").and_then(Value::as_str) {
                                if !text.trim().is_empty() {
                                    parts.push(text.trim().to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        parts.join("\n\n")
    }
}

#[async_trait]
impl Connector for XaiSearchConnector {
    fn name(&self) -> &'static str {
        "xai-search"
    }

    fn credential_provider(&self) -> &'static str {
        "xai"
    }

    fn description(&self) -> &'static str {
        "Search the web and X (Twitter) via xAI Responses API tools with citations."
    }

    fn display_name(&self) -> &'static str {
        "xAI Search"
    }

    fn icon(&self) -> &'static str {
        "xai"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["search", "ai", "web"]
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
                version: "0.1.0".into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Use 'search' to query web/X with xAI Responses API tools (web_search/x_search)."
                    .into(),
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
        let tool = Tool {
            name: Cow::Borrowed("search"),
            title: None,
            description: Some(Cow::Borrowed(
                "Search via xAI Responses API tools (web and/or X). Supports x_search handle/date/image/video filters. Use when you need up-to-date info. \
Example: query=\"today's Bitcoin price\" sources=[\"web\"] limit=5.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "User query"},
                    "sources": {
                        "type": "array",
                        "description": "Sources to search: web, x",
                        "items": {"type": "string", "enum": ["web", "x"]},
                        "default": ["web"]
                    },
                    "mode": {"type": "string", "enum": ["auto", "on", "off"], "default": "auto", "description": "Search mode"},
                    "limit": {"type": "integer", "default": 5, "description": "Approximate citations to include (default 5)."},
                    "max_results": {"type": "integer", "description": "Alias for limit (deprecated)."},
                    "model": {"type": "string", "description": "xAI model (e.g., grok-4-fast)"},
                    "language": {"type": "string", "description": "BCP-47 language hint (e.g., en)"},
                    "region": {"type": "string", "description": "Region/country code (e.g., US)"},
                    "since": {"type": "string", "description": "Earliest date (YYYY-MM-DD)"},
                    "until": {"type": "string", "description": "Latest date (YYYY-MM-DD)"},
                    "from_date": {"type": "string", "description": "x_search alias for since (YYYY-MM-DD)"},
                    "to_date": {"type": "string", "description": "x_search alias for until (YYYY-MM-DD)"},
                    "include_domains": {"type": "array", "items": {"type": "string"}},
                    "exclude_domains": {"type": "array", "items": {"type": "string"}},
                    "date_preset": {"type": "string", "description": "last_24_hours|last_7_days|last_30_days|this_month|past_year"},
                    "locale": {"type": "string", "description": "Locale like en-US or fr-FR"},
                    "allowed_x_handles": {"type": "array", "items": {"type": "string"}, "description": "x_search handles to include (max 10)"},
                    "excluded_x_handles": {"type": "array", "items": {"type": "string"}, "description": "x_search handles to exclude (max 10)"},
                    "enable_image_understanding": {"type": "boolean", "description": "Enable image understanding for X search"},
                    "enable_video_understanding": {"type": "boolean", "description": "Enable video understanding for X search"},
                    "response_format": {"type": "string", "enum": ["concise","detailed"], "default": "concise"}
                },
                "required": ["query"],
                "additionalProperties": false
            }).as_object().expect("Schema object").clone()),
            output_schema: None,
            annotations: None,
            icons: None,
        };
        Ok(ListToolsResult {
            tools: vec![tool],
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        if request.name.as_ref() != "search" {
            return Err(ConnectorError::ToolNotFound);
        }
        let args = request.arguments.unwrap_or_default();
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ConnectorError::InvalidParams("Missing 'query'".into()))?;
        let sources: Vec<String> = args
            .get("sources")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(|x| x.to_string()))
                    .collect()
            })
            .unwrap_or_else(|| vec!["web".to_string()]);
        let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("auto");
        let limit = args
            .get("limit")
            .or_else(|| args.get("max_results"))
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;
        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_model);
        let detailed = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s == "detailed")
            .unwrap_or(false);
        let filters = resolve_search_filters(&args);
        let x_options = Self::resolve_x_search_options(&args, &filters)?;

        let key = self.api_key.as_ref().ok_or_else(|| {
            ConnectorError::InvalidInput(
                "Missing credentials: set XAI_API_KEY or run `rzn-tools config set xai-search --value <key>`."
                    .into(),
            )
        })?;
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", key))
                .map_err(|e| ConnectorError::Other(e.to_string()))?,
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let filters_clause = Self::build_x_search_prompt_clause(&args, &filters, &x_options);

        let body = json!({
            "model": model,
            "input": format!("Use search tools to answer and cite ~{} sources.\nQuestion: {}{}", limit, query, filters_clause),
            "tools": Self::build_tools(&sources, mode, limit, &filters, &x_options),
            "tool_choice": "auto"
        });

        let resp = self
            .client
            .post("https://api.x.ai/v1/responses")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "xAI API error: {} - {}",
                status, value
            )));
        }

        let answer = Self::extract_output_text(&value);

        let citations = value.get("citations").cloned().unwrap_or_else(|| json!([]));

        let mut data = json!({
            "provider": "xai",
            "model": model,
            "query": query,
            "sources": sources,
            "limit_hint": limit,
            "answer": answer,
            "citations": citations
        });
        if let Some(usage) = value.get("usage") {
            data["usage"] = usage.clone();
        }
        if detailed {
            data["raw"] = value.clone();
        }
        Ok(structured_result_with_text(&data, None)?)
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
        let mut auth = AuthDetails::new();
        if let Some(v) = &self.api_key {
            auth.insert("api_key".into(), v.clone());
        }
        auth.insert("model".into(), self.default_model.clone());
        Ok(auth)
    }
    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.api_key = details
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("XAI_API_KEY").ok());
        if let Some(m) = details.get("model").cloned() {
            self.default_model = m;
        }
        Ok(())
    }
    async fn test_auth(&self) -> Result<(), ConnectorError> {
        if self
            .api_key
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
        {
            Ok(())
        } else {
            Err(ConnectorError::InvalidInput("Missing api_key".into()))
        }
    }
    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "api_key".into(),
                    label: "xAI API Key".into(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some("Set XAI_API_KEY".into()),
                    options: None,
                },
                Field {
                    name: "model".into(),
                    label: "Default Model".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("e.g., grok-4-fast".into()),
                    options: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_from(pairs: &[(&str, Value)]) -> serde_json::Map<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    fn empty_filters() -> crate::utils::SearchFilters {
        crate::utils::SearchFilters {
            language: None,
            region: None,
            since: None,
            until: None,
            include_domains: vec![],
            exclude_domains: vec![],
        }
    }

    #[test]
    fn resolve_x_search_options_prefers_aliases_and_flags() {
        let args = map_from(&[
            ("allowed_x_handles", json!(["one", "two"])),
            ("from_date", json!("2025-10-01")),
            ("to_date", json!("2025-10-10")),
            ("enable_image_understanding", json!(true)),
            ("enable_video_understanding", json!(true)),
        ]);

        let options = XaiSearchConnector::resolve_x_search_options(&args, &empty_filters())
            .expect("options should resolve");

        assert_eq!(options.allowed_x_handles, vec!["one", "two"]);
        assert_eq!(options.from_date.as_deref(), Some("2025-10-01"));
        assert_eq!(options.to_date.as_deref(), Some("2025-10-10"));
        assert!(options.enable_image_understanding);
        assert!(options.enable_video_understanding);
    }

    #[test]
    fn resolve_x_search_options_rejects_conflicting_handle_filters() {
        let args = map_from(&[
            ("allowed_x_handles", json!(["one"])),
            ("excluded_x_handles", json!(["two"])),
        ]);

        let err = XaiSearchConnector::resolve_x_search_options(&args, &empty_filters())
            .expect_err("conflicting handle filters should fail");

        match err {
            ConnectorError::InvalidParams(msg) => {
                assert!(msg.contains("allowed_x_handles") || msg.contains("excluded_x_handles"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn build_tools_propagates_x_search_filters() {
        let filters = crate::utils::SearchFilters {
            language: Some("en".to_string()),
            region: Some("US".to_string()),
            since: Some("2025-10-01".to_string()),
            until: Some("2025-10-10".to_string()),
            include_domains: vec!["example.com".to_string()],
            exclude_domains: vec!["bad.example".to_string()],
        };
        let options = XSearchOptions {
            allowed_x_handles: vec!["elonmusk".to_string()],
            excluded_x_handles: vec![],
            from_date: Some("2025-10-02".to_string()),
            to_date: Some("2025-10-09".to_string()),
            enable_image_understanding: true,
            enable_video_understanding: true,
        };

        let tools = XaiSearchConnector::build_tools(
            &["web".to_string(), "x".to_string()],
            "auto",
            5,
            &filters,
            &options,
        );

        let web_tool = tools
            .iter()
            .find(|tool| tool.get("type").and_then(Value::as_str) == Some("web_search"))
            .expect("web tool");
        assert_eq!(
            web_tool.get("enable_image_understanding"),
            Some(&json!(true))
        );

        let x_tool = tools
            .iter()
            .find(|tool| tool.get("type").and_then(Value::as_str) == Some("x_search"))
            .expect("x tool");
        assert_eq!(x_tool.get("allowed_x_handles"), Some(&json!(["elonmusk"])));
        assert_eq!(x_tool.get("from_date"), Some(&json!("2025-10-02")));
        assert_eq!(x_tool.get("to_date"), Some(&json!("2025-10-09")));
        assert_eq!(x_tool.get("enable_image_understanding"), Some(&json!(true)));
        assert_eq!(x_tool.get("enable_video_understanding"), Some(&json!(true)));
    }
}
