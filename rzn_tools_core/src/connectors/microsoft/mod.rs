use async_trait::async_trait;
use rmcp::model::*;
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;

// Bring the SDK into scope behind feature to ensure the crate is referenced when enabled.
#[allow(unused_imports)]
use graph_rs_sdk as graph;
#[allow(unused_imports)]
use graph_rs_sdk::prelude::Graph;

use crate::auth::AuthDetails;
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{collect_paginated_with_cursor, structured_result_with_text, Page};
use crate::Connector;
use crate::{
    auth_store::{AuthStore, FileAuthStore},
    oauth,
};
#[allow(unused_imports)]
use graph_rs_sdk::http::traits::AsyncIterator;
#[allow(unused_imports)]
use graph_rs_sdk::http::NextSession;

#[derive(Clone, Default)]
pub struct GraphConnector {
    auth: AuthDetails,
}

impl GraphConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self { auth })
    }

    async fn access_token(&self) -> Result<String, ConnectorError> {
        let store = FileAuthStore::new_default();
        let mut auth = store.load(self.name()).unwrap_or_else(|| self.auth.clone());
        if !auth.contains_key("access_token") && !self.auth.contains_key("access_token") {
            return Err(ConnectorError::Authentication(
                "Microsoft Graph auth not configured".into(),
            ));
        }
        // Merge self.auth over stored values
        for (k, v) in self.auth.iter() {
            auth.entry(k.clone()).or_insert(v.clone());
        }
        let token = crate::oauth::ensure_ms_access(&mut auth)?;
        let _ = store.save(self.name(), &auth);
        Ok(token)
    }
}

#[async_trait]
impl Connector for GraphConnector {
    fn name(&self) -> &'static str {
        "microsoft-graph"
    }

    fn description(&self) -> &'static str {
        "Microsoft 365 via Microsoft Graph: Outlook Mail/Calendar, SharePoint/Drive, and Teams (scaffold)."
    }

    fn display_name(&self) -> &'static str {
        "Microsoft Graph"
    }

    fn icon(&self) -> &'static str {
        "microsoft"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["productivity", "email", "calendar", "storage"]
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
                "Authenticate with Azure Entra ID. Supports delegated (device code/PKCE) and app creds (client credentials)."
                    .to_string(),
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
            Tool {
                name: Cow::Borrowed("list_messages"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List Outlook messages (requires explicit user permission).",
                )),
                input_schema: Arc::new(
	                    json!({
	                        "type": "object",
	                        "properties": {
	                            "top": { "type": "integer", "description": "Total messages to return (default 10, max 5000). Connector paginates internally.", "minimum": 1, "maximum": 5000 },
	                            "next_link": { "type": "string", "description": "Optional cursor from a previous response (@odata.nextLink)." }
	                        ,
	                            "response_format": { "type": "string", "enum": ["concise","detailed"], "description": "Default concise." }
	                        },
	                        "required": []
	}).as_object().expect("Schema object").clone()
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_events"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List Outlook calendar events (requires explicit user permission).",
                )),
                input_schema: Arc::new(
	                    json!({
	                        "type": "object",
	                        "properties": {
	                            "days_ahead": { "type": "integer", "description": "Window in days", "minimum": 1, "maximum": 30 }
	                            ,
	                            "limit": { "type": "integer", "description": "Total events to return (default 25, max 5000). Connector paginates internally.", "minimum": 1, "maximum": 5000 },
	                            "next_link": { "type": "string", "description": "Optional cursor from a previous response (@odata.nextLink)." }
	                        ,
	                            "response_format": { "type": "string", "enum": ["concise","detailed"], "description": "Default concise." }
	                        },
	                        "required": []
	}).as_object().expect("Schema object").clone()
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool { name: Cow::Borrowed("get_message"), title: None, description: Some(Cow::Borrowed("Get a message by ID (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"message_id":{"type":"string"}},"required":["message_id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("send_mail"), title: None, description: Some(Cow::Borrowed("Send email (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"to":{"type":"array","items":{"type":"string"}},"subject":{"type":"string"},"body_text":{"type":"string"}},"required":["to","subject","body_text"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("create_draft"), title: None, description: Some(Cow::Borrowed("Create draft email (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"to":{"type":"array","items":{"type":"string"}},"subject":{"type":"string"},"body_text":{"type":"string"}},"required":["to","subject","body_text"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("upload_attachment_large"), title: None, description: Some(Cow::Borrowed("Upload attachment to draft (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"message_id":{"type":"string"},"filename":{"type":"string"},"mime_type":{"type":"string"},"data_base64":{"type":"string"}},"required":["message_id","filename","mime_type","data_base64"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("send_draft"), title: None, description: Some(Cow::Borrowed("Send draft email (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"message_id":{"type":"string"}},"required":["message_id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool { name: Cow::Borrowed("upload_attachment_large_from_path"), title: None, description: Some(Cow::Borrowed("Upload attachment from file path (requires explicit user permission).")), input_schema: Arc::new(json!({"type":"object","properties":{"message_id":{"type":"string"},"file_path":{"type":"string"},"filename":{"type":"string"},"mime_type":{"type":"string"}},"required":["message_id","file_path"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None },
            Tool {
                name: Cow::Borrowed("auth_start"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Start device authorization (returns user_code and verification URL).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "tenant_id": {"type":"string"},
                            "client_id": {"type":"string"},
                            "scopes": {"type":"string", "description": "space-separated, e.g. Mail.Read Calendars.Read"}
                        }
                    }).as_object().expect("Schema object").clone()
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("auth_poll"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Poll token endpoint for device flow using device_code.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "tenant_id": {"type":"string"},
                            "client_id": {"type":"string"},
                            "device_code": {"type":"string"}
                        },
                        "required":["client_id","device_code"]
                    }).as_object().expect("Schema object").clone()
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
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
        let args = request.arguments.unwrap_or_default();
        match request.name.as_ref() {
            #[cfg(feature = "llm-macros")]
            "send_with_attachments" => {
                let to = args.get("to").and_then(|v| v.as_array()).ok_or(
                    crate::error::ConnectorError::InvalidParams(
                        "to must be array of emails".into(),
                    ),
                )?;
                let to_list: Vec<String> = to
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                let subject = args
                    .get("subject")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let body_text = args
                    .get("body_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let token = self.access_token().await?;
                let client = graph_rs_sdk::prelude::Graph::new(&token);
                let to_recipients: Vec<serde_json::Value> = to_list
                    .into_iter()
                    .map(|email| serde_json::json!({"emailAddress": {"address": email}}))
                    .collect();
                let payload = serde_json::json!({"subject": subject, "body": {"contentType": "Text", "content": body_text}, "toRecipients": to_recipients});
                let resp = client
                    .v1()
                    .me()
                    .messages()
                    .create_messages(&payload)
                    .send()
                    .map_err(|e| {
                        crate::error::ConnectorError::Other(format!(
                            "graph create draft error: {}",
                            e
                        ))
                    })?;
                let v: serde_json::Value = resp.into_body();
                let message_id = v
                    .get("id")
                    .and_then(|x| x.as_str())
                    .ok_or(crate::error::ConnectorError::Other(
                        "missing message id".into(),
                    ))?
                    .to_string();
                if let Some(atts) = args.get("attachments").and_then(|v| v.as_array()) {
                    let async_client = graph_rs_sdk::prelude::Graph::new_async(&token);
                    for a in atts {
                        let fp = a.get("file_path").and_then(|v| v.as_str());
                        let (file_path, size, name, mime) = if let Some(path) = fp {
                            let meta = std::fs::metadata(path).map_err(|e| {
                                crate::error::ConnectorError::Other(format!("stat file: {}", e))
                            })?;
                            let name = a
                                .get("filename")
                                .and_then(|v| v.as_str())
                                .or_else(|| {
                                    std::path::Path::new(path)
                                        .file_name()
                                        .and_then(|s| s.to_str())
                                })
                                .unwrap_or("attachment.bin")
                                .to_string();
                            let mime = a
                                .get("mime_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("application/octet-stream")
                                .to_string();
                            (path.to_string(), meta.len(), name, mime)
                        } else {
                            let data_b64 = a.get("data_base64").and_then(|v| v.as_str()).ok_or(
                                crate::error::ConnectorError::InvalidParams(
                                    "attachment requires data_base64 or file_path".into(),
                                ),
                            )?;
                            let mime = a
                                .get("mime_type")
                                .and_then(|v| v.as_str())
                                .unwrap_or("application/octet-stream")
                                .to_string();
                            let name = a
                                .get("filename")
                                .and_then(|v| v.as_str())
                                .unwrap_or("attachment.bin")
                                .to_string();
                            use base64::Engine as _;
                            let bytes = base64::engine::general_purpose::STANDARD
                                .decode(data_b64)
                                .or_else(|_| {
                                    base64::engine::general_purpose::URL_SAFE.decode(data_b64)
                                })
                                .map_err(|e| {
                                    crate::error::ConnectorError::InvalidParams(format!(
                                        "base64 decode: {}",
                                        e
                                    ))
                                })?;
                            let tmp_path = std::env::temp_dir().join(format!(
                                "rzn_ms_att_{}_{}.bin",
                                &name,
                                (chrono::Utc::now()
                                    .timestamp_nanos_opt()
                                    .unwrap_or(chrono::Utc::now().timestamp_millis() * 1_000_000))
                            ));
                            std::fs::write(&tmp_path, &bytes).map_err(|e| {
                                crate::error::ConnectorError::Other(format!("write temp: {}", e))
                            })?;
                            (
                                tmp_path.to_string_lossy().to_string(),
                                bytes.len() as u64,
                                name,
                                mime,
                            )
                        };
                        let body = serde_json::json!({"AttachmentItem": {"attachmentType": "file", "name": name, "size": size, "contentType": mime}});
                        let mut session = async_client
                            .v1()
                            .me()
                            .message(&message_id)
                            .attachments()
                            .create_upload_session(&file_path, &body)
                            .send()
                            .await
                            .map_err(|e| {
                                crate::error::ConnectorError::Other(format!(
                                    "graph create upload session: {}",
                                    e
                                ))
                            })?;
                        while let Some(next) = session.next().await {
                            match next {
                                Ok(graph_rs_sdk::http::NextSession::Next(_)) => {}
                                Ok(graph_rs_sdk::http::NextSession::Done(_)) => break,
                                Err(e) => {
                                    return Err(crate::error::ConnectorError::Other(format!(
                                        "upload error: {}",
                                        e
                                    )));
                                }
                            }
                        }
                    }
                }
                client
                    .v1()
                    .me()
                    .message(&message_id)
                    .send()
                    .send()
                    .map_err(|e| {
                        crate::error::ConnectorError::Other(format!(
                            "graph send draft error: {}",
                            e
                        ))
                    })?;
                return crate::utils::structured_result_with_text(
                    &serde_json::json!({"status":"sent","message_id": message_id}),
                    None,
                );
            }

            "list_messages" => {
                let desired = args
                    .get("top")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(10)
                    .clamp(1, 5_000) as usize;
                let start_link = args
                    .get("next_link")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let token = self.access_token().await?;

                let http = reqwest::Client::new();
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let collected = collect_paginated_with_cursor(
                    desired,
                    100,
                    start_link,
                    |cursor, remaining| {
                        let token = token.clone();
                        let http = http.clone();
                        async move {
                            let per_page = (remaining as i32).clamp(1, 50);
                            let v: serde_json::Value = if let Some(next) = cursor {
                                http.get(next)
                                    .bearer_auth(&token)
                                    .send()
                                    .await
                                    .map_err(ConnectorError::HttpRequest)?
                                    .json()
                                    .await
                                    .map_err(ConnectorError::HttpRequest)?
                            } else {
                                let client = Graph::new(&token);
                                let resp = client
                                    .v1()
                                    .me()
                                    .messages()
                                    .list_messages()
                                    .top(&(per_page.to_string()))
                                    .send()
                                    .map_err(|e| {
                                        ConnectorError::Other(format!("graph error: {}", e))
                                    })?;
                                resp.into_body()
                            };

                            let items = v
                                .get("value")
                                .and_then(|vv| vv.as_array())
                                .cloned()
                                .unwrap_or_default()
                                .into_iter()
                                .map(|m| {
                                    if concise {
                                        let id = m
                                            .get("id")
                                            .and_then(|x| x.as_str())
                                            .unwrap_or_default();
                                        let subject =
                                            m.get("subject").and_then(|x| x.as_str()).unwrap_or("");
                                        let rcv =
                                            m.get("receivedDateTime").and_then(|x| x.as_str());
                                        let (from_name, from_addr) = (
                                            m.get("from")
                                                .and_then(|f| f.get("emailAddress"))
                                                .and_then(|e| e.get("name"))
                                                .and_then(|s| s.as_str())
                                                .unwrap_or(""),
                                            m.get("from")
                                                .and_then(|f| f.get("emailAddress"))
                                                .and_then(|e| e.get("address"))
                                                .and_then(|s| s.as_str())
                                                .unwrap_or(""),
                                        );
                                        let from = if from_name.is_empty() {
                                            from_addr.to_string()
                                        } else {
                                            format!("{} <{}>", from_name, from_addr)
                                        };
                                        serde_json::json!({
                                            "id": id,
                                            "subject": subject,
                                            "from": from,
                                            "receivedDateTime": rcv
                                        })
                                    } else {
                                        m
                                    }
                                })
                                .collect::<Vec<_>>();

                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: v
                                    .get("@odata.nextLink")
                                    .and_then(|s| s.as_str())
                                    .map(str::to_string),
                            })
                        }
                    },
                    |m: &serde_json::Value| {
                        m.get("id").and_then(|x| x.as_str()).map(str::to_string)
                    },
                )
                .await?;

                let v = serde_json::json!({
                    "messages": collected.items,
                    "nextLink": collected.next_cursor
                });
                structured_result_with_text(&v, None)
            }
            "list_events" => {
                let token = self.access_token().await?;

                let desired = args
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(25)
                    .clamp(1, 5_000) as usize;
                let start_link = args
                    .get("next_link")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);

                let http = reqwest::Client::new();
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let collected = collect_paginated_with_cursor(
                    desired,
                    100,
                    start_link,
                    |cursor, remaining| {
                        let token = token.clone();
                        let http = http.clone();
                        async move {
                            let per_page = (remaining as i32).clamp(1, 50);
                            let v: serde_json::Value = if let Some(next) = cursor {
                                http.get(next)
                                    .bearer_auth(&token)
                                    .send()
                                    .await
                                    .map_err(ConnectorError::HttpRequest)?
                                    .json()
                                    .await
                                    .map_err(ConnectorError::HttpRequest)?
                            } else {
                                let client = Graph::new(&token);
                                let resp = client
                                    .v1()
                                    .me()
                                    .events()
                                    .list_events()
                                    .top(&(per_page.to_string()))
                                    .send()
                                    .map_err(|e| {
                                        ConnectorError::Other(format!("graph error: {}", e))
                                    })?;
                                resp.into_body()
                            };

                            let items = v
                                .get("value")
                                .and_then(|vv| vv.as_array())
                                .cloned()
                                .unwrap_or_default()
                                .into_iter()
                                .map(|e| {
                                    if concise {
                                        let id = e
                                            .get("id")
                                            .and_then(|x| x.as_str())
                                            .unwrap_or_default();
                                        let subject =
                                            e.get("subject").and_then(|x| x.as_str()).unwrap_or("");
                                        let start = e
                                            .get("start")
                                            .and_then(|t| t.get("dateTime"))
                                            .and_then(|s| s.as_str());
                                        let end = e
                                            .get("end")
                                            .and_then(|t| t.get("dateTime"))
                                            .and_then(|s| s.as_str());
                                        serde_json::json!({
                                            "id": id,
                                            "subject": subject,
                                            "start": start,
                                            "end": end
                                        })
                                    } else {
                                        e
                                    }
                                })
                                .collect::<Vec<_>>();

                            Ok::<_, ConnectorError>(Page {
                                items,
                                next_cursor: v
                                    .get("@odata.nextLink")
                                    .and_then(|s| s.as_str())
                                    .map(str::to_string),
                            })
                        }
                    },
                    |e: &serde_json::Value| {
                        e.get("id").and_then(|x| x.as_str()).map(str::to_string)
                    },
                )
                .await?;

                let v = serde_json::json!({
                    "events": collected.items,
                    "nextLink": collected.next_cursor
                });
                structured_result_with_text(&v, None)
            }
            "get_message" => {
                let message_id = args.get("message_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("message_id is required".to_string()),
                )?;
                let token = self.access_token().await?;
                let client = Graph::new(&token);
                let resp = client
                    .v1()
                    .me()
                    .message(message_id)
                    .get_messages()
                    .send()
                    .map_err(|e| ConnectorError::Other(format!("graph error: {}", e)))?;
                let v: serde_json::Value = resp.into_body();
                structured_result_with_text(&v, None)
            }
            "send_mail" => {
                let to = args.get("to").and_then(|v| v.as_array()).ok_or(
                    ConnectorError::InvalidParams("to must be array of emails".to_string()),
                )?;
                let to_list: Vec<String> = to
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                if to_list.is_empty() {
                    return Err(ConnectorError::InvalidParams(
                        "at least one recipient is required".to_string(),
                    ));
                }
                let subject = args
                    .get("subject")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let body_text = args
                    .get("body_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let token = self.access_token().await?;
                let client = Graph::new(&token);
                let to_recipients: Vec<serde_json::Value> = to_list
                    .into_iter()
                    .map(|email| json!({"emailAddress": {"address": email}}))
                    .collect();
                let atts = args
                    .get("attachments")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| {
                                let fname = a.get("filename").and_then(|v| v.as_str())?;
                                let ctype = a
                                    .get("mime_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("application/octet-stream");
                                let data_b64 = a.get("data_base64").and_then(|v| v.as_str())?;
                                Some(json!({
                                    "@odata.type": "#microsoft.graph.fileAttachment",
                                    "name": fname,
                                    "contentType": ctype,
                                    "contentBytes": data_b64
                                }))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let message = if atts.is_empty() {
                    json!({"subject": subject, "body": {"contentType": "Text", "content": body_text}, "toRecipients": to_recipients})
                } else {
                    json!({"subject": subject, "body": {"contentType": "Text", "content": body_text}, "toRecipients": to_recipients, "attachments": atts})
                };
                let payload = json!({"message": message, "saveToSentItems": true});
                client
                    .v1()
                    .me()
                    .send_mail(&payload)
                    .send()
                    .map_err(|e| ConnectorError::Other(format!("graph sendMail error: {}", e)))?;
                structured_result_with_text(&json!({"status":"sent"}), None)
            }
            "create_draft" => {
                let to = args.get("to").and_then(|v| v.as_array()).ok_or(
                    ConnectorError::InvalidParams("to must be array of emails".to_string()),
                )?;
                let to_list: Vec<String> = to
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect();
                let subject = args
                    .get("subject")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let body_text = args
                    .get("body_text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let token = self.access_token().await?;
                let client = Graph::new(&token);
                let to_recipients: Vec<serde_json::Value> = to_list
                    .into_iter()
                    .map(|email| json!({"emailAddress": {"address": email}}))
                    .collect();
                let payload = json!({"subject": subject, "body": {"contentType": "Text", "content": body_text}, "toRecipients": to_recipients});
                let resp = client
                    .v1()
                    .me()
                    .messages()
                    .create_messages(&payload)
                    .send()
                    .map_err(|e| {
                        ConnectorError::Other(format!("graph create draft error: {}", e))
                    })?;
                let v: serde_json::Value = resp.into_body();
                let id = v.get("id").cloned().unwrap_or(serde_json::Value::Null);
                structured_result_with_text(&json!({"message_id": id}), None)
            }
            "upload_attachment_large" => {
                use base64::Engine as _;
                let message_id = args.get("message_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("message_id is required".to_string()),
                )?;
                let filename = args.get("filename").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("filename is required".to_string()),
                )?;
                let mime_type = args
                    .get("mime_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream");
                let data_b64 = args.get("data_base64").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("data_base64 is required".to_string()),
                )?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data_b64)
                    .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(data_b64))
                    .map_err(|e| ConnectorError::InvalidParams(format!("base64 decode: {}", e)))?;
                let tmp_path = std::env::temp_dir().join(format!(
                    "rzn_ms_att_{}_{}.bin",
                    message_id,
                    (chrono::Utc::now()
                        .timestamp_nanos_opt()
                        .unwrap_or(chrono::Utc::now().timestamp_millis() * 1_000_000))
                ));
                std::fs::write(&tmp_path, &bytes)
                    .map_err(|e| ConnectorError::Other(format!("write tmp: {}", e)))?;
                let size = bytes.len() as u64;
                drop(bytes);
                let store = FileAuthStore::new_default();
                let auth = store.load(self.name()).ok_or_else(|| {
                    ConnectorError::Authentication("No tokens stored".to_string())
                })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let async_client = graph_rs_sdk::prelude::Graph::new_async(&token);
                let body = json!({"AttachmentItem": {"attachmentType": "file", "name": filename, "size": size, "contentType": mime_type}});
                let mut session = async_client
                    .v1()
                    .me()
                    .message(message_id)
                    .attachments()
                    .create_upload_session(&tmp_path, &body)
                    .send()
                    .await
                    .map_err(|e| {
                        ConnectorError::Other(format!("graph create upload session: {}", e))
                    })?;
                while let Some(next) = session.next().await {
                    match next {
                        Ok(NextSession::Next(_)) => { /* continue */ }
                        Ok(NextSession::Done(_)) => break,
                        Err(e) => {
                            let _ = std::fs::remove_file(&tmp_path);
                            return Err(ConnectorError::Other(format!("upload error: {}", e)));
                        }
                    }
                }
                let _ = std::fs::remove_file(&tmp_path);
                structured_result_with_text(&json!({"status":"uploaded"}), None)
            }
            "upload_attachment_large_from_path" => {
                let message_id = args.get("message_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("message_id is required".to_string()),
                )?;
                let file_path = args.get("file_path").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("file_path is required".to_string()),
                )?;
                let filename = args.get("filename").and_then(|v| v.as_str()).or_else(|| {
                    std::path::Path::new(file_path)
                        .file_name()
                        .and_then(|s| s.to_str())
                });
                let mime_type = args
                    .get("mime_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream");
                let meta = std::fs::metadata(file_path)
                    .map_err(|e| ConnectorError::Other(format!("stat file: {}", e)))?;
                let size = meta.len();
                let store = FileAuthStore::new_default();
                let auth = store.load(self.name()).ok_or_else(|| {
                    ConnectorError::Authentication("No tokens stored".to_string())
                })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let async_client = graph_rs_sdk::prelude::Graph::new_async(&token);
                let name = filename.unwrap_or("attachment.bin");
                let body = json!({"AttachmentItem": {"attachmentType": "file", "name": name, "size": size, "contentType": mime_type}});
                let mut session = async_client
                    .v1()
                    .me()
                    .message(message_id)
                    .attachments()
                    .create_upload_session(file_path, &body)
                    .send()
                    .await
                    .map_err(|e| {
                        ConnectorError::Other(format!("graph create upload session: {}", e))
                    })?;
                while let Some(next) = session.next().await {
                    match next {
                        Ok(NextSession::Next(_)) => { /* continue */ }
                        Ok(NextSession::Done(_)) => break,
                        Err(e) => {
                            return Err(ConnectorError::Other(format!("upload error: {}", e)));
                        }
                    }
                }
                structured_result_with_text(&json!({"status":"uploaded"}), None)
            }
            "send_draft" => {
                let message_id = args.get("message_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("message_id is required".to_string()),
                )?;
                let store = FileAuthStore::new_default();
                let auth = store.load(self.name()).ok_or_else(|| {
                    ConnectorError::Authentication("No tokens stored".to_string())
                })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = Graph::new(&token);
                client
                    .v1()
                    .me()
                    .message(message_id)
                    .send()
                    .send()
                    .map_err(|e| ConnectorError::Other(format!("graph send draft error: {}", e)))?;
                structured_result_with_text(&json!({"status":"sent"}), None)
            }
            "auth_start" => {
                let tenant = args.get("tenant_id").and_then(|v| v.as_str()).unwrap_or("");
                let client_id = args.get("client_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("client_id is required".to_string()),
                )?;
                let scopes = args
                    .get("scopes")
                    .and_then(|v| v.as_str())
                    .unwrap_or("offline_access Mail.Read Calendars.Read Files.Read");
                let start = oauth::ms_device_authorize(tenant, client_id, scopes).await?;
                let text = serde_json::to_string(&start)?;
                structured_result_with_text(&start, Some(text))
            }
            "auth_poll" => {
                let tenant = args.get("tenant_id").and_then(|v| v.as_str()).unwrap_or("");
                let client_id = args.get("client_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("client_id is required".to_string()),
                )?;
                let device_code = args.get("device_code").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("device_code is required".to_string()),
                )?;
                let tokens = oauth::ms_device_poll(tenant, client_id, device_code).await?;
                let mut auth = self.auth.clone();
                auth.insert("access_token".to_string(), tokens.access_token.clone());
                if let Some(r) = tokens.refresh_token.clone() {
                    auth.insert("refresh_token".to_string(), r);
                }
                if let Some(e) = tokens.expires_in {
                    auth.insert("expires_in".to_string(), e.to_string());
                }
                if crate::oauth_client::should_persist_tokens() {
                    let store = FileAuthStore::new_default();
                    let _ = store.save(self.name(), &auth);
                }
                let text = serde_json::to_string(&tokens)?;
                structured_result_with_text(&tokens, Some(text))
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
        Err(ConnectorError::InvalidParams(
            "Prompt not found".to_string(),
        ))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(self.auth.clone())
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.auth = details;
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // For now, require some hint of config to mark ready; else needs_auth.
        let store = FileAuthStore::new_default();
        let auth = store.load(self.name()).unwrap_or_default();
        if auth.contains_key("access_token") || self.auth.contains_key("access_token") {
            return Ok(());
        }
        Err(ConnectorError::Authentication(
            "Microsoft Graph auth not configured".to_string(),
        ))
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "tenant_id".to_string(),
                    label: "Tenant ID".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Azure Entra tenant ID (optional for common).".to_string()),
                    options: None,
                },
                Field {
                    name: "client_id".to_string(),
                    label: "Client ID".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("App registration client ID.".to_string()),
                    options: None,
                },
                Field {
                    name: "client_secret".to_string(),
                    label: "Client Secret".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("Required for client credentials flow.".to_string()),
                    options: None,
                },
                Field {
                    name: "auth_method".to_string(),
                    label: "Auth Method".to_string(),
                    field_type: FieldType::Select {
                        options: vec![
                            "device_code".to_string(),
                            "pkce".to_string(),
                            "client_credentials".to_string(),
                        ],
                    },
                    required: false,
                    description: Some("Choose the OAuth flow for local dev or server.".to_string()),
                    options: None,
                },
                Field {
                    name: "scopes".to_string(),
                    label: "Scopes".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Space-separated scopes, e.g. Mail.Read Calendars.Read Files.Read"
                            .to_string(),
                    ),
                    options: None,
                },
            ],
        }
    }
}
