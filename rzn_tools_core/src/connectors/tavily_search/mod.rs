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

pub struct TavilySearchConnector {
    client: Client,
    api_key: Option<String>,
}

impl TavilySearchConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.1.0")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        let api_key = auth
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("TAVILY_API_KEY").ok());
        Ok(Self { client, api_key })
    }
}

#[async_trait]
impl Connector for TavilySearchConnector {
    fn name(&self) -> &'static str {
        "tavily-search"
    }
    fn description(&self) -> &'static str {
        "Tavily Search API — fast, blended web/news search with summaries."
    }

    fn display_name(&self) -> &'static str {
        "Tavily Search"
    }

    fn icon(&self) -> &'static str {
        "tavily"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["search", "web"]
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
        Ok(InitializeResult { protocol_version: ProtocolVersion::LATEST, capabilities: self.capabilities().await, server_info: Implementation { name: self.name().into(), version: "0.1.0".into(), title: None, icons: None, website_url: None }, instructions: Some("Use 'search' with topic (general|news), depth (basic|advanced) and include_answer.".into()) })
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
        let tool = Tool { name: Cow::Borrowed("search"), title: None, description: Some(Cow::Borrowed("Web/news search via Tavily. Use when you want recent sources (topic=news) or broad web discovery (topic=general). Example: query=\"rust async\" topic=\"general\" limit=5.")), input_schema: Arc::new(json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "topic": {"type": "string", "enum": ["general","news"], "default":"general"},
                "depth": {"type": "string", "enum": ["basic","advanced"], "default":"basic", "description": "Search depth"},
                "limit": {"type": "integer", "default": 10},
                "max_results": {"type": "integer", "description": "Alias for limit (deprecated)."},
                "include_answer": {"type": "boolean", "default": true},
                "include_images": {"type": "boolean", "default": false},
                "include_domains": {"type": "array", "items": {"type": "string"}},
                "exclude_domains": {"type": "array", "items": {"type": "string"}},
                "date_preset": {"type": "string", "description": "last_24_hours|last_7_days|last_30_days|this_month|past_year"},
                "locale": {"type": "string", "description": "Locale like en-US or fr-FR"},
                "response_format": {"type": "string", "enum": ["concise","detailed"], "default": "concise"}
            },
            "required": ["query"],
            "additionalProperties": false
        }).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None };
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
        let topic = args
            .get("topic")
            .and_then(|v| v.as_str())
            .unwrap_or("general");
        let depth = args
            .get("depth")
            .and_then(|v| v.as_str())
            .unwrap_or("basic");
        let max_results = args
            .get("limit")
            .or_else(|| args.get("max_results"))
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;
        let include_answer = args
            .get("include_answer")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let include_images = args
            .get("include_images")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
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

        let key = self.api_key.as_ref().ok_or_else(|| {
            ConnectorError::InvalidInput(
                "Missing credentials: set TAVILY_API_KEY or run `rzn-tools config set tavily-search --value <key>`."
                    .into(),
            )
        })?;
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        let mut body = json!({
            "api_key": key,
            "query": query,
            "topic": topic,
            "search_depth": depth,
            "max_results": max_results,
            "include_answer": include_answer,
            "include_images": include_images
        });
        if let Some(v) = include_domains {
            body["include_domains"] = json!(v);
        }
        if let Some(v) = exclude_domains {
            body["exclude_domains"] = json!(v);
        }

        let resp = self
            .client
            .post("https://api.tavily.com/search")
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;
        let detailed = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s == "detailed")
            .unwrap_or(false);
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "Tavily API error: {} - {}",
                status, value
            )));
        }

        let mut data = json!({
            "provider": "tavily",
            "query": query,
            "topic": topic,
            "depth": depth,
            "max_results": max_results,
            "answer": value.get("answer").cloned().unwrap_or(Value::Null),
            "results": value.get("results").cloned().unwrap_or_else(|| json!([]))
        });
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
            .or_else(|| std::env::var("TAVILY_API_KEY").ok());
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
            fields: vec![Field {
                name: "api_key".into(),
                label: "Tavily API Key".into(),
                field_type: FieldType::Secret,
                required: true,
                description: Some("Set TAVILY_API_KEY".into()),
                options: None,
            }],
        }
    }
}
