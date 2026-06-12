use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{build_filters_clause, resolve_search_filters, structured_result_with_text};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

pub struct AnthropicWebSearchConnector {
    client: Client,
    api_key: Option<String>,
    default_model: String,
}

impl AnthropicWebSearchConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.1.0")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        let api_key = auth
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());
        let default_model = auth
            .get("model")
            .cloned()
            .unwrap_or_else(|| "claude-3-7-sonnet-latest".to_string());

        Ok(Self {
            client,
            api_key,
            default_model,
        })
    }

    fn build_headers(&self) -> Result<HeaderMap, ConnectorError> {
        let mut headers = HeaderMap::new();
        let key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ConnectorError::InvalidInput("Anthropic api_key not set".into()))?;
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(key).map_err(|e| ConnectorError::Other(e.to_string()))?,
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        Ok(headers)
    }
}

#[async_trait]
impl Connector for AnthropicWebSearchConnector {
    fn name(&self) -> &'static str {
        "anthropic-search"
    }

    fn credential_provider(&self) -> &'static str {
        "anthropic"
    }
    fn description(&self) -> &'static str {
        "Search the web via Claude Web Search tool."
    }

    fn display_name(&self) -> &'static str {
        "Anthropic Search"
    }

    fn icon(&self) -> &'static str {
        "anthropic"
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
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
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
            instructions: Some("Use the search tool to query the web via Anthropic.".into()),
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
                "Grounded web search via Anthropic. Use when you need up-to-date facts and \
sources. Example: query=\"latest US inflation print\" limit=5.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "description": "Approximate number of sources (default 5).", "default": 5},
                    "max_results": {"type": "integer", "description": "Alias for limit (deprecated)."},
                    "model": {"type": "string", "description": "Claude model (e.g., claude-3-7-sonnet-latest)"},
                    "max_output_tokens": {"type": "integer"},
                    "language": {"type": "string", "description": "BCP-47 language hint (e.g., en)"},
                    "region": {"type": "string", "description": "Region/country code (e.g., US)"},
                    "since": {"type": "string", "description": "Earliest date (YYYY-MM-DD)"},
                    "until": {"type": "string", "description": "Latest date (YYYY-MM-DD)"},
                    "include_domains": {"type": "array", "items": {"type": "string"}},
                    "exclude_domains": {"type": "array", "items": {"type": "string"}},
                    "date_preset": {"type": "string", "description": "last_24_hours|last_7_days|last_30_days|this_month|past_year"},
                    "locale": {"type": "string", "description": "Locale like en-US or fr-FR"},
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
        let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
            ConnectorError::InvalidParams(
                "'query' is required (string). Example: What changed in SEC climate rules in 2025?"
                    .into(),
            )
        })?;
        let limit = args
            .get("limit")
            .or_else(|| args.get("max_results"))
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;
        let model = args
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.default_model);
        let max_tokens = args
            .get("max_output_tokens")
            .or_else(|| args.get("max_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1024);
        let detailed = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s == "detailed")
            .unwrap_or(false);
        let filters = resolve_search_filters(&args);
        let filters_clause = build_filters_clause(&filters);

        let headers = self.build_headers()?;
        let body = json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": [ { "role": "user", "content": format!("Use web search to answer with citations (~{}). Question: {}{}", limit, query, filters_clause) } ],
            "tools": [ { "type": "web_search_20250305" } ],
            "tool_choice": { "type": "auto" }
        });

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Anthropic API error: {} - {}",
                status, value
            )));
        }

        // Aggregate text parts
        let answer = value
            .get("content")
            .and_then(|c| c.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        let citations = value.get("citations").cloned().unwrap_or_else(|| json!([]));

        let mut data = json!({
            "provider": "anthropic",
            "model": model,
            "query": query,
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
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());
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
                    label: "Anthropic API Key".into(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some("Set ANTHROPIC_API_KEY or provide here".into()),
                    options: None,
                },
                Field {
                    name: "model".into(),
                    label: "Default Model".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("e.g., claude-3-7-sonnet-latest".into()),
                    options: None,
                },
            ],
        }
    }
}
