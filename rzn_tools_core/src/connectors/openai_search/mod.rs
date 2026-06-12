use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{build_filters_clause, resolve_search_filters, structured_result_with_text};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

pub struct OpenAIWebSearchConnector {
    client: Client,
    api_key: Option<String>,
    org: Option<String>,
    project: Option<String>,
    default_model: String,
}

impl OpenAIWebSearchConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.1.0")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        let api_key = auth
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok());
        let org = auth
            .get("organization")
            .cloned()
            .or_else(|| std::env::var("OPENAI_ORG_ID").ok());
        let project = auth
            .get("project")
            .cloned()
            .or_else(|| std::env::var("OPENAI_PROJECT_ID").ok());
        let default_model = auth
            .get("model")
            .cloned()
            .unwrap_or_else(|| "o4-mini".to_string());

        Ok(Self {
            client,
            api_key,
            org,
            project,
            default_model,
        })
    }

    fn build_headers(&self) -> Result<HeaderMap, ConnectorError> {
        let mut headers = HeaderMap::new();
        let key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ConnectorError::InvalidInput("OpenAI api_key not set".into()))?;
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", key))
                .map_err(|e| ConnectorError::Other(e.to_string()))?,
        );
        if let Some(org) = &self.org {
            headers.insert(
                "OpenAI-Organization",
                HeaderValue::from_str(org).map_err(|e| ConnectorError::Other(e.to_string()))?,
            );
        }
        if let Some(project) = &self.project {
            headers.insert(
                "OpenAI-Project",
                HeaderValue::from_str(project).map_err(|e| ConnectorError::Other(e.to_string()))?,
            );
        }
        Ok(headers)
    }
}

#[async_trait]
impl Connector for OpenAIWebSearchConnector {
    fn name(&self) -> &'static str {
        "openai-search"
    }

    fn credential_provider(&self) -> &'static str {
        "openai"
    }

    fn description(&self) -> &'static str {
        "Search the web via OpenAI Responses API built-in web_search tool."
    }

    fn display_name(&self) -> &'static str {
        "OpenAI Search"
    }

    fn icon(&self) -> &'static str {
        "openai"
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
            instructions: Some("Use the search tool to query the web via OpenAI.".to_string()),
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
        Ok(vec![])
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tool = Tool {
            name: Cow::Borrowed("search"),
            title: None,
            description: Some(Cow::Borrowed(
                "Grounded web search via OpenAI. Use when you need up-to-date facts and \
sources. Example: query=\"What changed in SEC climate rules in 2025?\" limit=5.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "User question or query"},
                    "limit": {"type": "integer", "description": "Approximate number of sources to cite (default 5).", "default": 5},
                    "max_results": {"type": "integer", "description": "Alias for limit (deprecated)."},
                    "model": {"type": "string", "description": "Model name (e.g., o4-mini, gpt-4.1)"},
                    "max_output_tokens": {"type": "integer", "description": "Max tokens for model output"},
                    "language": {"type": "string", "description": "BCP-47 language hint (e.g., en)"},
                    "region": {"type": "string", "description": "Region/country code (e.g., US)"},
                    "since": {"type": "string", "description": "Earliest date (YYYY-MM-DD)"},
                    "until": {"type": "string", "description": "Latest date (YYYY-MM-DD)"},
                    "include_domains": {"type": "array", "items": {"type": "string"}},
                    "exclude_domains": {"type": "array", "items": {"type": "string"}},
                    "date_preset": {"type": "string", "description": "last_24_hours|last_7_days|last_30_days|this_month|past_year"},
                    "locale": {"type": "string", "description": "Locale like en-US or fr-FR"},
                    "response_format": {"type": "string", "enum": ["concise","detailed"], "default": "concise", "description": "Concise omits raw payload; detailed includes it"}
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
            .and_then(|v| v.as_u64());
        let detailed = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s == "detailed")
            .unwrap_or(false);

        let filters = resolve_search_filters(&args);
        let filters_clause = build_filters_clause(&filters);

        let headers = self.build_headers()?;
        let mut body = json!({
            "model": model,
            "input": format!("Question: {}", query),
            "instructions": format!("Use web search to answer and cite ~{} high-quality sources with URLs.{}", limit, filters_clause),
            "tools": [{"type": "web_search"}],
            "tool_choice": "auto",
        });
        if let Some(mt) = max_tokens {
            body["max_output_tokens"] = json!(mt);
        }

        let resp = self
            .client
            .post("https://api.openai.com/v1/responses")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "OpenAI API error: {} - {}",
                status, value
            )));
        }

        let answer = value
            .get("output_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Try to surface citations if present under a common field
        let citations = value.get("citations").cloned().unwrap_or_else(|| json!([]));

        let mut data = json!({
            "provider": "openai",
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
        _request: Option<PaginatedRequestParam>,
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
        if let Some(v) = &self.org {
            auth.insert("organization".into(), v.clone());
        }
        if let Some(v) = &self.project {
            auth.insert("project".into(), v.clone());
        }
        auth.insert("model".into(), self.default_model.clone());
        Ok(auth)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.api_key = details
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok());
        self.org = details
            .get("organization")
            .cloned()
            .or_else(|| std::env::var("OPENAI_ORG_ID").ok());
        self.project = details
            .get("project")
            .cloned()
            .or_else(|| std::env::var("OPENAI_PROJECT_ID").ok());
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
                    label: "OpenAI API Key".into(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some("Set OPENAI_API_KEY or provide here".into()),
                    options: None,
                },
                Field {
                    name: "organization".into(),
                    label: "Organization ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Optional OpenAI-Organization header".into()),
                    options: None,
                },
                Field {
                    name: "project".into(),
                    label: "Project ID".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Optional OpenAI-Project header".into()),
                    options: None,
                },
                Field {
                    name: "model".into(),
                    label: "Default Model".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("e.g., o4-mini, gpt-4.1".into()),
                    options: None,
                },
            ],
        }
    }
}
