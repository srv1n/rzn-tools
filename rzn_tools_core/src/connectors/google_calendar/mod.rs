use async_trait::async_trait;
use rmcp::model::*;
use std::borrow::Cow;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{collect_paginated_with_cursor, structured_result_with_text, Page};
use crate::Connector;
#[allow(unused_imports)]
use google_calendar3 as calendar3;

pub struct GoogleCalendarConnector {
    auth: AuthDetails,
}
impl GoogleCalendarConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self { auth })
    }
}

#[async_trait]
impl Connector for GoogleCalendarConnector {
    fn name(&self) -> &'static str {
        "google-calendar"
    }
    fn description(&self) -> &'static str {
        "Google Calendar connector (list events)."
    }

    fn display_name(&self) -> &'static str {
        "Google Calendar"
    }

    fn icon(&self) -> &'static str {
        "google_calendar"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["productivity", "calendar"]
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
        Ok(InitializeResult { protocol_version: ProtocolVersion::LATEST, capabilities: self.capabilities().await, server_info: Implementation { name: self.name().to_string(), title: None, version: "0.1.0".to_string(), icons: None, website_url: None }, instructions: Some("Authenticate via Google device flow; shares tokens with other Google connectors under 'google-common'.".to_string()) })
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
        let mut tools = vec![
Tool { name: Cow::Borrowed("list_events"), title: None, description: Some(Cow::Borrowed("List events (requires explicit user permission).")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"max_results":{"type":"integer","minimum":1,"maximum":5000},"page_token":{"type":"string","description":"Optional cursor from a previous response (nextPageToken)."},"time_min":{"type":"string","description":"RFC3339"},"response_format":{"type":"string","enum":["concise","detailed"],"description":"Default concise."}},"required":[]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
        ];
        tools.push(Tool { name: std::borrow::Cow::Borrowed("create_event"), title: None, description: Some(std::borrow::Cow::Borrowed("Create an event (requires explicit user permission).")), input_schema: std::sync::Arc::new(serde_json::json!({"type":"object","properties":{"summary":{"type":"string"},"start":{"type":"string","description":"RFC3339"},"end":{"type":"string","description":"RFC3339"}},"required":["summary","start","end"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: std::borrow::Cow::Borrowed("sync_events"), title: None, description: Some(std::borrow::Cow::Borrowed("Incremental sync (requires explicit user permission).")), input_schema: std::sync::Arc::new(serde_json::json!({"type":"object","properties":{"sync_token":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":250}} ,"required":["sync_token"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: std::borrow::Cow::Borrowed("update_event"), title: None, description: Some(std::borrow::Cow::Borrowed("Update an event (requires explicit user permission).")), input_schema: std::sync::Arc::new(serde_json::json!({"type":"object","properties":{"event_id":{"type":"string"},"summary":{"type":"string"},"start":{"type":"string","description":"RFC3339"},"end":{"type":"string","description":"RFC3339"}},"required":["event_id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool {
            name: std::borrow::Cow::Borrowed("delete_event"),
            title: None,
            description: Some(std::borrow::Cow::Borrowed(
                "Delete an event (requires explicit user permission).",
            )),
            input_schema: std::sync::Arc::new(
                serde_json::json!({"type":"object","properties":{"event_id":{"type":"string"}}})
                    .as_object()
                    .expect("Schema object")
                    .clone(),
            ),
            output_schema: None,
            annotations: None,
            icons: None,
        });
        tools.push(Tool { name: std::borrow::Cow::Borrowed("watch_events"), title: None, description: Some(std::borrow::Cow::Borrowed("Start calendar webhook (requires explicit user permission).")), input_schema: std::sync::Arc::new(serde_json::json!({"type":"object","properties":{"address":{"type":"string"},"id":{"type":"string"},"token":{"type":"string"}},"required":["address"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: std::borrow::Cow::Borrowed("stop_channel"), title: None, description: Some(std::borrow::Cow::Borrowed("Stop webhook channel (requires explicit user permission).")), input_schema: std::sync::Arc::new(serde_json::json!({"type":"object","properties":{"id":{"type":"string"},"resource_id":{"type":"string"}},"required":["id","resource_id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        if !crate::oauth_client::admin_tools_enabled() {
            tools.retain(|t| {
                let n = t.name.as_ref();
                !matches!(n, "watch_events" | "stop_channel")
            });
        }
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }
    async fn call_tool(&self, req: CallToolRequestParam) -> Result<CallToolResult, ConnectorError> {
        let args = req.arguments.unwrap_or_default();
        match req.name.as_ref() {
            "list_events" => {
                let max = args
                    .get("max_results")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(25);
                let start_token = args
                    .get("page_token")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let time_min = args
                    .get("time_min")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-calendar")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = calendar3::CalendarHub::new(client, token.clone());
                let time_min_dt = chrono::DateTime::parse_from_rfc3339(&time_min)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());
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
                        async move {
                            let per_page = (remaining as i32).clamp(1, 250);
                            let mut call = hub
                                .events()
                                .list("primary")
                                .single_events(true)
                                .order_by("startTime")
                                .time_min(time_min_dt)
                                .max_results(per_page);
                            if let Some(t) = cursor {
                                call = call.page_token(&t);
                            }
                            let (_, events) = call.doit().await.map_err(|e| {
                                ConnectorError::Other(format!("calendar error: {}", e))
                            })?;

                            let items = events
                                .items
                                .unwrap_or_default()
                                .into_iter()
                                .map(|ev| {
                                    if concise {
                                        serde_json::json!({
                                            "id": ev.id.unwrap_or_default(),
                                            "summary": ev.summary.unwrap_or_default(),
                                            "start": ev.start.and_then(|t| t.date_time.map(|d| d.to_rfc3339())),
                                            "end": ev.end.and_then(|t| t.date_time.map(|d| d.to_rfc3339())),
                                        })
                                    } else {
                                        serde_json::to_value(&ev).unwrap_or(serde_json::json!({}))
                                    }
                                })
                                .collect::<Vec<_>>();

                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: events.next_page_token,
                            })
                        }
                    },
                    |e: &serde_json::Value| e.get("id").and_then(|v| v.as_str()).map(str::to_string),
                )
                .await?;

                let v = serde_json::json!({
                    "events": collected.items,
                    "nextPageToken": collected.next_cursor
                });
                structured_result_with_text(&v, None)
            }
            "create_event" => {
                let summary = args.get("summary").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("summary is required".to_string()),
                )?;
                let start_str = args.get("start").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("start is required".to_string()),
                )?;
                let end_str = args
                    .get("end")
                    .and_then(|v| v.as_str())
                    .ok_or(ConnectorError::InvalidParams("end is required".to_string()))?;
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-calendar")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = calendar3::CalendarHub::new(client, token.clone());
                let start_dt = chrono::DateTime::parse_from_rfc3339(start_str)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .map_err(|e| ConnectorError::InvalidParams(format!("invalid start: {}", e)))?;
                let end_dt = chrono::DateTime::parse_from_rfc3339(end_str)
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .map_err(|e| ConnectorError::InvalidParams(format!("invalid end: {}", e)))?;
                let ev = calendar3::api::Event {
                    summary: Some(summary.to_string()),
                    start: Some(calendar3::api::EventDateTime {
                        date_time: Some(start_dt),
                        ..Default::default()
                    }),
                    end: Some(calendar3::api::EventDateTime {
                        date_time: Some(end_dt),
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                let (_, created) = hub
                    .events()
                    .insert(ev, "primary")
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("calendar insert error: {}", e)))?;
                let v = serde_json::to_value(&created)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "sync_events" => {
                let sync_token = args.get("sync_token").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("sync_token is required".to_string()),
                )?;
                let max = args
                    .get("max_results")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(250);
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-calendar")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = calendar3::CalendarHub::new(client, token.clone());
                let (_, events) = hub
                    .events()
                    .list("primary")
                    .sync_token(sync_token)
                    .max_results(max as i32)
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("calendar sync error: {}", e)))?;
                let v = serde_json::to_value(&events)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "update_event" => {
                let event_id = args.get("event_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("event_id is required".to_string()),
                )?;
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-calendar")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = calendar3::CalendarHub::new(client, token.clone());
                let mut ev = calendar3::api::Event::default();
                if let Some(s) = args.get("summary").and_then(|v| v.as_str()) {
                    ev.summary = Some(s.to_string());
                }
                if let Some(start) = args.get("start").and_then(|v| v.as_str()) {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(start) {
                        ev.start = Some(calendar3::api::EventDateTime {
                            date_time: Some(dt.with_timezone(&chrono::Utc)),
                            ..Default::default()
                        });
                    }
                }
                if let Some(end) = args.get("end").and_then(|v| v.as_str()) {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(end) {
                        ev.end = Some(calendar3::api::EventDateTime {
                            date_time: Some(dt.with_timezone(&chrono::Utc)),
                            ..Default::default()
                        });
                    }
                }
                let (_, updated) = hub
                    .events()
                    .patch(ev, "primary", event_id)
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("calendar patch error: {}", e)))?;
                let v = serde_json::to_value(&updated)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "delete_event" => {
                let event_id = args.get("event_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("event_id is required".to_string()),
                )?;
                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-calendar")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = calendar3::CalendarHub::new(client, token.clone());
                let _ = hub
                    .events()
                    .delete("primary", event_id)
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("calendar delete error: {}", e)))?;
                structured_result_with_text(&serde_json::json!({"status":"deleted"}), None)
            }
            "watch_events" => {
                let address = args.get("address").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("address (webhook URL) is required".to_string()),
                )?;
                let channel_id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
                let token_param = args.get("token").and_then(|v| v.as_str());

                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-calendar")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let access_token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = calendar3::CalendarHub::new(client, access_token.clone());

                let mut channel = calendar3::api::Channel {
                    id: Some(channel_id.clone()),
                    type_: Some("web_hook".to_string()),
                    address: Some(address.to_string()),
                    ..Default::default()
                };
                if let Some(t) = token_param {
                    channel.token = Some(t.to_string());
                }

                let (_, result) = hub
                    .events()
                    .watch(channel, "primary")
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("calendar watch error: {}", e)))?;

                let v = serde_json::to_value(&result)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "stop_channel" => {
                let channel_id = args.get("id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("id (channel id) is required".to_string()),
                )?;
                let resource_id = args.get("resource_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("resource_id is required".to_string()),
                )?;

                let store = FileAuthStore::new_default();
                let auth = store
                    .load("google-calendar")
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let access_token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = calendar3::CalendarHub::new(client, access_token.clone());

                let channel = calendar3::api::Channel {
                    id: Some(channel_id.to_string()),
                    resource_id: Some(resource_id.to_string()),
                    ..Default::default()
                };

                let _ = hub.channels().stop(channel).doit().await.map_err(|e| {
                    ConnectorError::Other(format!("calendar stop channel error: {}", e))
                })?;

                structured_result_with_text(
                    &serde_json::json!({"status": "stopped", "channel_id": channel_id}),
                    None,
                )
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
            .load("google-calendar")
            .or_else(|| store.load("google-common"))
            .unwrap_or_default();
        if auth.contains_key("access_token") || self.auth.contains_key("access_token") {
            Ok(())
        } else {
            Err(ConnectorError::Authentication(
                "Calendar auth not configured".to_string(),
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
                    "Use scope https://www.googleapis.com/auth/calendar.readonly".to_string(),
                ),
                options: None,
            }],
        }
    }
}
