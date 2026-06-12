use async_trait::async_trait;
use rmcp::model::*;
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{collect_paginated_with_cursor, structured_result_with_text, Page};
use crate::Connector;
#[allow(unused_imports)]
use google_people1 as people1;

pub struct GooglePeopleConnector {
    auth: AuthDetails,
}
impl GooglePeopleConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self { auth })
    }
}

#[async_trait]
impl Connector for GooglePeopleConnector {
    fn name(&self) -> &'static str {
        "google-people"
    }
    fn description(&self) -> &'static str {
        "Google People API (contacts)."
    }

    fn display_name(&self) -> &'static str {
        "Google People"
    }

    fn icon(&self) -> &'static str {
        "google_people"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["productivity", "contacts"]
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
                "Authenticate via Google device flow; shares tokens with other Google connectors."
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
            Tool {
                name: Cow::Borrowed("list_connections"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List contacts (requires explicit user permission).",
                )),
                input_schema: Arc::new(json!({"type":"object","properties":{"page_size":{"type":"integer","minimum":1,"maximum":200},"limit":{"type":"integer","minimum":1,"maximum":5000,"description":"Total contacts to return (default: page_size). Connector paginates internally."},"page_token":{"type":"string","description":"Optional cursor from a previous response (nextPageToken)."},"response_format":{"type":"string","enum":["concise","detailed"]}},"required":[]}).as_object().expect("Schema object").clone()),
                output_schema: None, annotations: None, icons: None
            },
            Tool {
                name: Cow::Borrowed("get_person"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get contact by resourceName (requires explicit user permission).",
                )),
                input_schema: Arc::new(json!({"type":"object","properties":{"resource_name":{"type":"string"},"person_fields":{"type":"string","description":"Comma-separated fields, e.g. names,emailAddresses,phoneNumbers"},"response_format":{"type":"string","enum":["concise","detailed"]}},"required":["resource_name"]}).as_object().expect("Schema object").clone()),
                output_schema: None, annotations: None, icons: None
            },
        ];
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }
    async fn call_tool(&self, req: CallToolRequestParam) -> Result<CallToolResult, ConnectorError> {
        let args = req.arguments.unwrap_or_default();
        match req.name.as_ref() {
            "list_connections" => {
                let page_size = args.get("page_size").and_then(|v| v.as_i64()).unwrap_or(50);
                let desired_limit = args
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(page_size)
                    .clamp(1, 5_000) as usize;
                let start_token = args
                    .get("page_token")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-people")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = people1::PeopleService::new(client, token.clone());
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let page_size = page_size.clamp(1, 200) as usize;
                let collected = collect_paginated_with_cursor(
                    desired_limit,
                    100,
                    start_token,
                    |cursor, remaining| {
                        let hub = hub.clone();
                        async move {
                            let per_page = (remaining.min(page_size)).clamp(1, 200) as i32;
                            let mut call = hub
                                .people()
                                .connections_list("people/me")
                                .page_size(per_page)
                                .person_fields(people1::client::FieldMask::new(&[
                                    "names",
                                    "emailAddresses",
                                ]));
                            if let Some(t) = cursor {
                                call = call.page_token(&t);
                            }
                            let (_, cons) = call.doit().await.map_err(|e| {
                                ConnectorError::Other(format!("people error: {}", e))
                            })?;

                            let items = cons
                                .connections
                                .unwrap_or_default()
                                .into_iter()
                                .map(|p| {
                                    if concise {
                                        let rn = p.resource_name.unwrap_or_default();
                                        let name = p
                                            .names
                                            .as_ref()
                                            .and_then(|ns| ns.first())
                                            .and_then(|n| n.display_name.clone())
                                            .unwrap_or_default();
                                        let email = p
                                            .email_addresses
                                            .as_ref()
                                            .and_then(|es| es.first())
                                            .and_then(|e| e.value.clone());
                                        serde_json::json!({
                                            "resourceName": rn,
                                            "name": name,
                                            "email": email
                                        })
                                    } else {
                                        serde_json::to_value(&p).unwrap_or(json!({}))
                                    }
                                })
                                .collect::<Vec<_>>();

                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: cons.next_page_token,
                            })
                        }
                    },
                    |p: &serde_json::Value| {
                        p.get("resourceName")
                            .and_then(|v| v.as_str())
                            .map(str::to_string)
                    },
                )
                .await?;

                let v = serde_json::json!({
                    "people": collected.items,
                    "nextPageToken": collected.next_cursor
                });
                structured_result_with_text(&v, None)
            }

            "get_person" => {
                let resource_name = args.get("resource_name").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("resource_name is required".to_string()),
                )?;
                let fields = args
                    .get("person_fields")
                    .and_then(|v| v.as_str())
                    .unwrap_or("names,emailAddresses");
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-people")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = people1::PeopleService::new(client, token.clone());
                let mut call = hub.people().get(resource_name);
                call = call.person_fields(people1::client::FieldMask::new(
                    &fields.split(',').collect::<Vec<_>>(),
                ));
                let (_, person) = call
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("people get error: {}", e)))?;
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );
                if concise {
                    let rn = person.resource_name.clone().unwrap_or_default();
                    let name = person
                        .names
                        .as_ref()
                        .and_then(|ns| ns.first())
                        .and_then(|n| n.display_name.clone())
                        .unwrap_or_default();
                    let email = person
                        .email_addresses
                        .as_ref()
                        .and_then(|es| es.first())
                        .and_then(|e| e.value.clone());
                    let v = serde_json::json!({"resourceName": rn, "name": name, "email": email});
                    structured_result_with_text(&v, None)
                } else {
                    let v = serde_json::to_value(&person)
                        .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                    structured_result_with_text(&v, None)
                }
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
            .load("google-people")
            .or_else(|| store.load("google-common"))
            .unwrap_or_default();
        if auth.contains_key("access_token") || self.auth.contains_key("access_token") {
            Ok(())
        } else {
            Err(ConnectorError::Authentication(
                "People auth not configured".to_string(),
            ))
        }
    }
    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![Field {
                name: "scopes".to_string(),
                label: "Scopes".to_string(),
                field_type: FieldType::Text,
                required: false,
                description: Some(
                    "Use scope https://www.googleapis.com/auth/contacts.readonly".to_string(),
                ),
                options: None,
            }],
        }
    }
}
