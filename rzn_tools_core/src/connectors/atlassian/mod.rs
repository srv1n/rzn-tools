use async_trait::async_trait;
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;
use base64::Engine as _;

#[derive(Clone, Default)]
pub struct AtlassianConnector {
    auth: AuthDetails,
    client: reqwest::Client,
}

impl AtlassianConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let client = reqwest::Client::builder()
            .user_agent("rzn-datasourcer/0.1 atlassian-connector")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        Ok(Self { auth, client })
    }

    fn jira_base(&self) -> Option<String> {
        self.auth.get("jira_base").cloned().or_else(|| {
            FileAuthStore::new_default()
                .load(self.name())
                .and_then(|m| m.get("jira_base").cloned())
        })
    }
    fn confluence_base(&self) -> Option<String> {
        self.auth.get("confluence_base").cloned().or_else(|| {
            FileAuthStore::new_default()
                .load(self.name())
                .and_then(|m| m.get("confluence_base").cloned())
        })
    }
    fn user(&self) -> Option<String> {
        self.auth.get("user").cloned().or_else(|| {
            FileAuthStore::new_default()
                .load(self.name())
                .and_then(|m| m.get("user").cloned())
        })
    }
    fn token(&self) -> Option<String> {
        self.auth.get("token").cloned().or_else(|| {
            FileAuthStore::new_default()
                .load(self.name())
                .and_then(|m| m.get("token").cloned())
        })
    }

    fn basic_auth_header(&self) -> Result<String, ConnectorError> {
        let user = self.user().ok_or_else(|| {
            ConnectorError::Authentication("Atlassian email/user not configured".to_string())
        })?;
        let token = self.token().ok_or_else(|| {
            ConnectorError::Authentication("Atlassian API token not configured".to_string())
        })?;
        Ok(format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user, token))
        ))
    }

    async fn jira_get(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<Value, ConnectorError> {
        let base = self.jira_base().ok_or_else(|| {
            ConnectorError::Authentication("jira_base not configured".to_string())
        })?;
        let url = format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let auth = self.basic_auth_header()?;
        self.send_with_backoff(|client| {
            client
                .get(&url)
                .header(reqwest::header::AUTHORIZATION, auth.clone())
                .query(&params)
        })
        .await
    }

    async fn confluence_get(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<Value, ConnectorError> {
        let base = self.confluence_base().ok_or_else(|| {
            ConnectorError::Authentication("confluence_base not configured".to_string())
        })?;
        let url = format!(
            "{}/{}",
            base.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let auth = self.basic_auth_header()?;
        self.send_with_backoff(|client| {
            client
                .get(&url)
                .header(reqwest::header::AUTHORIZATION, auth.clone())
                .query(&params)
        })
        .await
    }

    async fn send_with_backoff<F>(&self, build: F) -> Result<Value, ConnectorError>
    where
        F: Fn(&reqwest::Client) -> reqwest::RequestBuilder,
    {
        use tokio::time::{sleep, Duration};
        const MAX_RETRIES: usize = 4;
        let mut delay_ms = 700u64;
        for attempt in 0..=MAX_RETRIES {
            let resp = build(&self.client)
                .try_clone()
                .unwrap_or_else(|| build(&self.client))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    if r.status().as_u16() == 429 {
                        // rate limit
                        let ra = r
                            .headers()
                            .get("Retry-After")
                            .and_then(|h| h.to_str().ok())
                            .and_then(|s| s.parse::<u64>().ok())
                            .unwrap_or(delay_ms);
                        if attempt == MAX_RETRIES {
                            return Err(ConnectorError::Other("Rate limited (429)".into()));
                        }
                        sleep(Duration::from_millis(ra * 1000)).await;
                        delay_ms = (delay_ms as f64 * 1.8) as u64;
                        continue;
                    }
                    if r.status().is_server_error() {
                        if attempt == MAX_RETRIES {
                            return Err(ConnectorError::Other(format!("HTTP {}", r.status())));
                        }
                        sleep(Duration::from_millis(delay_ms)).await;
                        delay_ms = (delay_ms as f64 * 1.6) as u64;
                        continue;
                    }
                    let v = r
                        .json::<Value>()
                        .await
                        .map_err(ConnectorError::HttpRequest)?;
                    return Ok(v);
                }
                Err(e) => {
                    if attempt == MAX_RETRIES {
                        return Err(ConnectorError::HttpRequest(e));
                    }
                    sleep(Duration::from_millis(delay_ms)).await;
                    delay_ms = (delay_ms as f64 * 1.6) as u64;
                    continue;
                }
            }
        }
        Err(ConnectorError::Other("request failed after retries".into()))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JiraSearchInput {
    jql: String,
    #[serde(default)]
    start_at: Option<u32>,
    #[serde(default)]
    max_results: Option<u32>,
    #[serde(default)]
    fields: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JiraGetIssueInput {
    key: String,
    #[serde(default)]
    expand: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConfluenceSearchInput {
    cql: String,
    #[serde(default)]
    start: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConfluenceGetPageInput {
    id: String,
    #[serde(default)]
    expand: Option<String>,
}

#[async_trait]
impl Connector for AtlassianConnector {
    fn name(&self) -> &'static str {
        "atlassian"
    }
    fn description(&self) -> &'static str {
        "Atlassian Cloud: Jira (issues/JQL) and Confluence (pages/search) via API token (basic auth)."
    }

    fn display_name(&self) -> &'static str {
        "Atlassian"
    }

    fn icon(&self) -> &'static str {
        "atlassian"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["productivity", "project-management", "developer"]
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
                "Set jira_base/confluence_base, user (email), and token (API token).".into(),
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
            Tool { name: Cow::Borrowed("test_auth"), title: None, description: Some(Cow::Borrowed("Validate Jira/Confluence auth by fetching self info.")), input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            // Jira
            Tool { name: Cow::Borrowed("jira_search_issues"), title: None, description: Some(Cow::Borrowed("Search issues with JQL.")), input_schema: Arc::new(json!({"type":"object","properties":{"jql":{"type":"string"},"start_at":{"type":"integer"},"max_results":{"type":"integer"},"fields":{"type":"string"}},"required":["jql"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("jira_get_issue"), title: None, description: Some(Cow::Borrowed("Get a Jira issue with optional expand.")), input_schema: Arc::new(json!({"type":"object","properties":{"key":{"type":"string"},"expand":{"type":"string"}},"required":["key"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            // Confluence
            Tool { name: Cow::Borrowed("conf_search_pages"), title: None, description: Some(Cow::Borrowed("Search Confluence with CQL.")), input_schema: Arc::new(json!({"type":"object","properties":{"cql":{"type":"string"},"start":{"type":"integer"},"limit":{"type":"integer"}},"required":["cql"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("conf_get_page"), title: None, description: Some(Cow::Borrowed("Get a Confluence page (view/storage) with expand.")), input_schema: Arc::new(json!({"type":"object","properties":{"id":{"type":"string"},"expand":{"type":"string"}},"required":["id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
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
        let name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();
        let args_map = serde_json::Map::from_iter(args);
        match name {
            "test_auth" => {
                // Jira myprofile
                let j = if self.jira_base().is_some() {
                    Some(self.jira_get("rest/api/3/myself", &[]).await?)
                } else {
                    None
                };
                let c = if self.confluence_base().is_some() {
                    Some(
                        self.confluence_get("wiki/rest/api/user/current", &[])
                            .await?,
                    )
                } else {
                    None
                };
                structured_result_with_text(&json!({"jira": j, "confluence": c}), None)
            }
            "jira_search_issues" => {
                let input: JiraSearchInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut params = vec![("jql", input.jql)];
                if let Some(s) = input.start_at {
                    params.push(("startAt", s.to_string()));
                }
                if let Some(m) = input.max_results {
                    params.push(("maxResults", m.to_string()));
                }
                if let Some(f) = input.fields {
                    params.push(("fields", f));
                }
                let v = self.jira_get("rest/api/3/search", &params).await?;
                structured_result_with_text(&v, None)
            }
            "jira_get_issue" => {
                let input: JiraGetIssueInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut params = vec![];
                if let Some(expand) = input.expand {
                    params.push(("expand", expand));
                }
                let v = self
                    .jira_get(&format!("rest/api/3/issue/{}", input.key), &params)
                    .await?;
                structured_result_with_text(&v, None)
            }
            "conf_search_pages" => {
                let input: ConfluenceSearchInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut params = vec![("cql", input.cql)];
                if let Some(s) = input.start {
                    params.push(("start", s.to_string()));
                }
                if let Some(l) = input.limit {
                    params.push(("limit", l.to_string()));
                }
                // Ask for view body by default for RAG friendliness
                params.push(("expand", "body.view,version,space,history".to_string()));
                let v = self.confluence_get("wiki/rest/api/search", &params).await?;
                structured_result_with_text(&v, None)
            }
            "conf_get_page" => {
                let input: ConfluenceGetPageInput = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut params = vec![];
                if let Some(expand) = input.expand {
                    params.push(("expand", expand));
                }
                let v = self
                    .confluence_get(&format!("wiki/rest/api/content/{}", input.id), &params)
                    .await?;
                structured_result_with_text(&v, None)
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
        Err(ConnectorError::InvalidParams("Prompt not found".into()))
    }
    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(self.auth.clone())
    }
    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.auth = details.clone();
        let _ = FileAuthStore::new_default().save(self.name(), &details);
        Ok(())
    }
    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let _ = self
            .call_tool(CallToolRequestParam {
                name: "test_auth".into(),
                arguments: Some(serde_json::Map::new()),
            })
            .await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "jira_base".into(),
                    label: "Jira Base URL".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("e.g., https://your-domain.atlassian.net".into()),
                    options: None,
                },
                Field {
                    name: "confluence_base".into(),
                    label: "Confluence Base URL".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("e.g., https://your-domain.atlassian.net".into()),
                    options: None,
                },
                Field {
                    name: "user".into(),
                    label: "Atlassian Email/User".into(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Account email for API token".into()),
                    options: None,
                },
                Field {
                    name: "token".into(),
                    label: "API Token".into(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("Create token at id.atlassian.com/manage/api-tokens".into()),
                    options: None,
                },
            ],
        }
    }
}
