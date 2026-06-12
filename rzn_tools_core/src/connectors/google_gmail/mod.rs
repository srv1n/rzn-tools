use async_trait::async_trait;
use rmcp::model::*;
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;

// official SDKs
use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{collect_paginated_with_cursor, structured_result_with_text, Page};
use crate::Connector;
#[allow(unused_imports)]
use google_gmail1 as gmail1;

pub struct GmailConnector {
    auth: AuthDetails,
}

impl GmailConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self { auth })
    }
}

#[async_trait]
impl Connector for GmailConnector {
    fn name(&self) -> &'static str {
        "google-gmail"
    }
    fn description(&self) -> &'static str {
        "Gmail connector (list messages)."
    }

    fn display_name(&self) -> &'static str {
        "Gmail"
    }

    fn icon(&self) -> &'static str {
        "gmail"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["productivity", "email"]
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
        _r: InitializeRequestParam,
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
                "Use Google device auth via Drive connector or share tokens under 'google-common'."
                    .to_string(),
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
        Err(ConnectorError::ResourceNotFound)
    }
    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool { name: Cow::Borrowed("list_messages"), title: None, description: Some(Cow::Borrowed("List messages (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"q":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":5000},"page_token":{"type":"string","description":"Optional cursor from a previous response (nextPageToken)."},"response_format":{"type":"string","enum":["concise","detailed"]}},"required":[]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("decode_message_raw"), title: None, description: Some(Cow::Borrowed("Decode a raw message (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"raw_base64url":{"type":"string"}},"required":["raw_base64url"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("get_message"), title: None, description: Some(Cow::Borrowed("Get a message by id (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"id":{"type":"string"},"format":{"type":"string"},"response_format":{"type":"string","enum":["concise","detailed"]}},"required":["id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("get_thread"), title: None, description: Some(Cow::Borrowed("Get a thread by id (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
        ];
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }
    async fn call_tool(&self, req: CallToolRequestParam) -> Result<CallToolResult, ConnectorError> {
        let args = req.arguments.unwrap_or_default();
        match req.name.as_ref() {
            "list_messages" => {
                let q = args.get("q").and_then(|v| v.as_str()).unwrap_or("");
                let max = args
                    .get("max_results")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(25);
                let start_token = args
                    .get("page_token")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-gmail")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = gmail1::Gmail::new(client, token.clone());

                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let desired = (max.max(1) as u32).clamp(1, 5_000) as usize;
                let collected = collect_paginated_with_cursor(
                    desired,
                    100,
                    start_token,
                    |cursor, remaining| {
                        let hub = hub.clone();
                        let q = q.to_string();
                        async move {
                            let per_page = (remaining as u32).clamp(1, 500);
                            let mut call = hub.users().messages_list("me").max_results(per_page);
                            if !q.is_empty() {
                                call = call.q(&q);
                            }
                            if let Some(t) = cursor {
                                call = call.page_token(&t);
                            }
                            let (_, list) = call.doit().await.map_err(|e| {
                                ConnectorError::Other(format!("gmail error: {}", e))
                            })?;

                            let msgs = list
                                .messages
                                .unwrap_or_default()
                                .into_iter()
                                .map(|m| {
                                    if concise {
                                        serde_json::json!({
                                            "id": m.id.unwrap_or_default(),
                                            "threadId": m.thread_id.unwrap_or_default()
                                        })
                                    } else {
                                        serde_json::to_value(&m).unwrap_or(json!({}))
                                    }
                                })
                                .collect::<Vec<_>>();

                            Ok::<_, ConnectorError>(Page {
                                items: msgs,
                                next_cursor: list.next_page_token,
                            })
                        }
                    },
                    |m: &serde_json::Value| {
                        m.get("id").and_then(|v| v.as_str()).map(str::to_string)
                    },
                )
                .await?;

                let v = serde_json::json!({
                    "messages": collected.items,
                    "nextPageToken": collected.next_cursor
                });
                structured_result_with_text(&v, None)
            }
            "get_message" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or(ConnectorError::InvalidParams("id is required".to_string()))?;
                let format = args.get("format").and_then(|v| v.as_str()).unwrap_or("raw");
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-gmail")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = gmail1::Gmail::new(client, token.clone());
                let mut call = hub.users().messages_get("me", id);
                if !format.is_empty() {
                    call = call.format(format);
                }
                let (_, msg) = call
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("gmail get error: {}", e)))?;
                let v = serde_json::to_value(&msg)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "get_thread" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or(ConnectorError::InvalidParams("id is required".to_string()))?;
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-gmail")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = gmail1::Gmail::new(client, token.clone());
                let (_, thread) = hub
                    .users()
                    .threads_get("me", id)
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("gmail thread error: {}", e)))?;
                let v = serde_json::to_value(&thread)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "decode_message_raw" => {
                let raw_base64url = args.get("raw_base64url").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("raw_base64url is required".to_string()),
                )?;

                // Decode base64url to bytes
                use base64::Engine;
                let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .decode(raw_base64url)
                    .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw_base64url))
                    .map_err(|e| {
                        ConnectorError::InvalidParams(format!("Invalid base64url: {}", e))
                    })?;

                // Convert to string (email is text-based)
                let email_str = String::from_utf8_lossy(&decoded);

                // Parse email headers and body
                let mut headers = serde_json::Map::new();
                let mut body = String::new();
                let mut in_headers = true;

                for line in email_str.lines() {
                    if in_headers {
                        if line.is_empty() {
                            in_headers = false;
                            continue;
                        }
                        if let Some(colon_pos) = line.find(':') {
                            let key = line[..colon_pos].trim().to_lowercase();
                            let value = line[colon_pos + 1..].trim();
                            headers.insert(key, serde_json::Value::String(value.to_string()));
                        }
                    } else {
                        if !body.is_empty() {
                            body.push('\n');
                        }
                        body.push_str(line);
                    }
                }

                let result = json!({
                    "headers": headers,
                    "body": body,
                    "raw_length": decoded.len()
                });
                structured_result_with_text(&result, None)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
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
        Err(ConnectorError::InvalidParams(
            "Prompt not found".to_string(),
        ))
    }
    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(self.auth.clone())
    }
    async fn set_auth_details(&mut self, d: AuthDetails) -> Result<(), ConnectorError> {
        self.auth = d;
        Ok(())
    }
    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let store = FileAuthStore::new_default();
        let auth = store
            .load("google-gmail")
            .or_else(|| store.load("google-common"))
            .unwrap_or_default();
        if auth.contains_key("access_token") || self.auth.contains_key("access_token") {
            Ok(())
        } else {
            Err(ConnectorError::Authentication(
                "Gmail auth not configured".to_string(),
            ))
        }
    }
    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema { fields: vec![Field { name: "scopes".to_string(), label: "Scopes".to_string(), field_type: FieldType::Text, required: false, description: Some("Use Drive connector auth_start with Gmail scopes: https://www.googleapis.com/auth/gmail.readonly".to_string()), options: None }] }
    }
}
