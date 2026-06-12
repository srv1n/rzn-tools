use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{build_filters_clause, resolve_search_filters, structured_result_with_text};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::Client;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

pub struct GeminiSearchConnector {
    client: Client,
    api_key: Option<String>,
    default_model: String,
}

impl GeminiSearchConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.1.0")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;

        let api_key = auth
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .or_else(|| std::env::var("GOOGLE_API_KEY").ok());
        let default_model = auth
            .get("model")
            .cloned()
            .unwrap_or_else(|| "gemini-2.5-pro".to_string());

        Ok(Self {
            client,
            api_key,
            default_model,
        })
    }
}

#[async_trait]
impl Connector for GeminiSearchConnector {
    fn name(&self) -> &'static str {
        "gemini-search"
    }

    fn credential_provider(&self) -> &'static str {
        "gemini"
    }

    fn description(&self) -> &'static str {
        "Search the web using Gemini's Google Search grounding."
    }

    fn display_name(&self) -> &'static str {
        "Gemini Search"
    }

    fn icon(&self) -> &'static str {
        "gemini"
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
                version: "0.1.0".into(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some("Enable google_search tool and ask a question.".into()),
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
                "Grounded web search via Gemini; use for current info. Example: query=\"FDA CRISPR approval\" limit=5.",
            )),
            input_schema: Arc::new(json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "default": 5},
                    "max_results": {"type": "integer", "description": "Alias for limit (deprecated)."},
                    "model": {"type": "string", "description": "Gemini model (e.g., gemini-1.5-pro-latest)"},
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
        let detailed = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s == "detailed")
            .unwrap_or(false);
        let filters = resolve_search_filters(&args);
        let filters_clause = build_filters_clause(&filters);

        // Respect explicit model override by rebuilding URL
        let url = {
            let key = self
                .api_key
                .as_ref()
                .ok_or_else(|| ConnectorError::InvalidInput("Gemini api_key not set".into()))?;
            format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model, key
            )
        };

        let body = json!({
            "contents": [ { "role": "user", "parts": [ { "text": format!("Use Google Search to answer with ~{} citations. Question: {}{}", limit, query, filters_clause) } ] } ],
            "tools": [ { "googleSearch": {} } ]
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Gemini API error: {} - {}",
                status, value
            )));
        }

        // Extract answer text from candidates[0].content.parts[].text
        let answer = value
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|c| c.get("content"))
            .and_then(|ct| ct.get("parts"))
            .and_then(|p| p.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        let mut data = json!({
            "provider": "google-gemini",
            "model": model,
            "query": query,
            "limit_hint": limit,
            "answer": answer,

        });
        if let Some(meta) = value.get("usageMetadata") {
            let usage = json!({
                "input_tokens": meta.get("promptTokenCount").cloned(),
                "output_tokens": meta.get("candidatesTokenCount").cloned(),
                "total_tokens": meta.get("totalTokenCount").cloned(),
            });
            data["usage"] = usage;
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
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .or_else(|| std::env::var("GOOGLE_API_KEY").ok());
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
                    label: "Gemini API Key".into(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some("Set GEMINI_API_KEY or GOOGLE_API_KEY".into()),
                    options: None,
                },
                Field {
                    name: "model".into(),
                    label: "Default Model".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("e.g., gemini-1.5-pro-latest".into()),
                    options: None,
                },
            ],
        }
    }
}
