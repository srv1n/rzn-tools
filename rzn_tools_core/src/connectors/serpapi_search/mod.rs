use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{resolve_search_filters, structured_result_with_text};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use reqwest::Client;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

pub struct SerpapiSearchConnector {
    client: Client,
    api_key: Option<String>,
}

impl SerpapiSearchConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = Client::builder()
            .user_agent("rzn-tools/0.1.0")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        let api_key = auth
            .get("api_key")
            .cloned()
            .or_else(|| std::env::var("SERPAPI_API_KEY").ok());
        Ok(Self { client, api_key })
    }
}

#[async_trait]
impl Connector for SerpapiSearchConnector {
    fn name(&self) -> &'static str {
        "serpapi-search"
    }
    fn description(&self) -> &'static str {
        "SerpAPI Google Search JSON (rich verticals, locality, filters)."
    }

    fn display_name(&self) -> &'static str {
        "SerpAPI Search"
    }

    fn icon(&self) -> &'static str {
        "serpapi"
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
                "Use 'search' with engine=google by default; supports hl, gl, location.".into(),
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
        let tool = Tool { name: Cow::Borrowed("search"), title: None, description: Some(Cow::Borrowed("SERP search via SerpAPI. Use when you need SERP-style results with locality/engine controls. Example: query=\"best rust linter\" limit=5 engine=\"google\".")), input_schema: Arc::new(json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "limit": {"type": "integer", "default": 10},
                "max_results": {"type": "integer", "description": "Alias for limit (deprecated)."},
                "engine": {"type": "string", "default": "google"},
                "hl": {"type": "string"},
                "gl": {"type": "string"},
                "location": {"type": "string"},
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
        let num = args
            .get("limit")
            .or_else(|| args.get("max_results"))
            .or_else(|| args.get("num"))
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;
        let detailed = args
            .get("response_format")
            .and_then(|v| v.as_str())
            .map(|s| s == "detailed")
            .unwrap_or(false);
        let engine = args
            .get("engine")
            .and_then(|v| v.as_str())
            .unwrap_or("google");
        let hl = args.get("hl").and_then(|v| v.as_str());
        let gl = args.get("gl").and_then(|v| v.as_str());
        let location = args.get("location").and_then(|v| v.as_str());
        let filters = resolve_search_filters(&args);

        let key = self.api_key.as_ref().ok_or_else(|| {
            ConnectorError::InvalidInput(
                "Missing credentials: set SERPAPI_API_KEY or run `rzn-tools config set serpapi-search --value <key>`."
                    .into(),
            )
        })?;

        let mut params: Vec<(String, String)> = vec![
            ("engine".into(), engine.to_string()),
            ("q".into(), query.to_string()),
            ("num".into(), num.to_string()),
            ("api_key".into(), key.to_string()),
        ];
        if let Some(v) = hl.or(filters.language.as_deref()) {
            params.push(("hl".into(), v.to_string()));
        }
        if let Some(v) = gl.or(filters.region.as_deref()) {
            params.push(("gl".into(), v.to_string()));
        }
        if let Some(v) = location {
            params.push(("location".into(), v.to_string()));
        }

        let resp = self
            .client
            .get("https://serpapi.com/search.json")
            .query(&params)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        let status = resp.status();
        let value: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;
        if !status.is_success() {
            return Err(ConnectorError::Other(format!(
                "SerpAPI error: {} - {}",
                status, value
            )));
        }

        let mut data = json!({ "provider": "serpapi", "query": query, "num": num, "engine": engine, "results": value });
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
            .or_else(|| std::env::var("SERPAPI_API_KEY").ok());
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
                label: "SerpAPI Key".into(),
                field_type: FieldType::Secret,
                required: true,
                description: Some("Set SERPAPI_API_KEY".into()),
                options: None,
            }],
        }
    }
}
