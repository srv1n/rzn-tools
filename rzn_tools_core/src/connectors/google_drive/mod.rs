use async_trait::async_trait;
use rmcp::model::*;
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;

// Ensure crates are referenced when feature is enabled
#[allow(unused_imports)]
use google_drive3 as drive3;
#[allow(unused_imports)]
use yup_oauth2 as oauth2;

use crate::auth::AuthDetails;
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::{collect_paginated_with_cursor, structured_result_with_text, Page};
use crate::Connector;
use crate::{
    auth_store::{AuthStore, FileAuthStore},
    oauth,
};
use crate::{URLParamExtraction, URLPatternSpec};
use base64::Engine as _;

#[derive(Clone, Default)]
pub struct DriveConnector {
    auth: AuthDetails,
}

impl DriveConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self { auth })
    }
}

#[async_trait]
impl Connector for DriveConnector {
    fn name(&self) -> &'static str {
        "google-drive"
    }

    fn description(&self) -> &'static str {
        "Google Drive connector (scaffold) for listing and fetching files via google-apis-rs."
    }

    fn display_name(&self) -> &'static str {
        "Google Drive"
    }

    fn icon(&self) -> &'static str {
        "google_drive"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["productivity", "storage"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![
            URLPatternSpec {
                pattern: r"(?:https?://)?drive\.google\.com/file/d/([A-Za-z0-9_-]+)".to_string(),
                default_tool: "get_file".to_string(),
                description: "Get Drive file metadata by ID".to_string(),
                param_extraction: vec![URLParamExtraction {
                    capture_group: 1,
                    param_name: "file_id".to_string(),
                    use_full_url: false,
                }],
            },
            URLPatternSpec {
                pattern: r"(?:https?://)?drive\.google\.com/open\?id=([A-Za-z0-9_-]+)".to_string(),
                default_tool: "get_file".to_string(),
                description: "Get Drive file metadata by ID".to_string(),
                param_extraction: vec![URLParamExtraction {
                    capture_group: 1,
                    param_name: "file_id".to_string(),
                    use_full_url: false,
                }],
            },
        ]
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
                "Authenticate with OAuth (installed app + PKCE). Provide client_id/client_secret and redirect URI."
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
        let mut tools: Vec<Tool> = Vec::new();
        tools.push(Tool { name: Cow::Borrowed("list_files"), title: None, description: Some(Cow::Borrowed("List Drive files (requires explicit user permission).")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"q":{"type":"string","description":"Drive query string"},"page_size":{"type":"integer","minimum":1,"maximum":100},"limit":{"type":"integer","minimum":1,"maximum":5000,"description":"Total number of files to return (default: page_size). Connector paginates internally."},"page_token":{"type":"string","description":"Optional cursor from a previous response (nextPageToken)."},"response_format":{"type":"string","enum":["concise","detailed"],"description":"Default concise."}},"required":[]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: Cow::Borrowed("get_file"), title: None, description: Some(Cow::Borrowed("Get file metadata (requires explicit user permission).")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"file_id":{"type":"string"},"response_format":{"type":"string","enum":["concise","detailed"]}},"required":["file_id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: Cow::Borrowed("download_file"), title: None, description: Some(Cow::Borrowed("Download file content (requires explicit user permission).")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"file_id":{"type":"string"},"max_bytes":{"type":"integer","description":"Optional cap to avoid huge responses"}},"required":["file_id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: Cow::Borrowed("export_file"), title: None, description: Some(Cow::Borrowed("Export Docs/Sheets/Slides (requires explicit user permission).")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"file_id":{"type":"string"},"mime_type":{"type":"string","description":"Target MIME type"}},"required":["file_id","mime_type"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: Cow::Borrowed("upload_file"), title: None, description: Some(Cow::Borrowed("Upload file via base64 (requires explicit user permission).")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"name":{"type":"string"},"mime_type":{"type":"string"},"data_base64":{"type":"string"},"parents":{"type":"array","items":{"type":"string"}}},"required":["name","mime_type","data_base64"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: Cow::Borrowed("upload_file_resumable"), title: None, description: Some(Cow::Borrowed("Resumable upload (requires explicit user permission).")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"name":{"type":"string"},"mime_type":{"type":"string"},"data_base64":{"type":"string"},"parents":{"type":"array","items":{"type":"string"}}},"required":["name","mime_type","data_base64"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        // Auth helpers
        tools.push(Tool { name: Cow::Borrowed("auth_start"), title: None, description: Some(Cow::Borrowed("Start device authorization for Google.")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"client_id":{"type":"string"},"scopes":{"type":"string","description":"space-separated scopes"}},"required":["client_id"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        tools.push(Tool { name: Cow::Borrowed("auth_poll"), title: None, description: Some(Cow::Borrowed("Poll token endpoint for device flow.")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"client_id":{"type":"string"},"client_secret":{"type":"string"},"device_code":{"type":"string"}},"required":["client_id","device_code"]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        // Composite tool (optional)
        if cfg!(feature = "llm-macros") {
            tools.push(Tool { name: Cow::Borrowed("find_and_export"), title: None, description: Some(Cow::Borrowed("Find and export a Doc/Sheet/Slide (requires explicit user permission).")), input_schema: Arc::new(serde_json::json!({"type":"object","properties":{"q":{"type":"string","description":"Drive query; ignored if file_id provided"},"file_id":{"type":"string","description":"Export this file directly instead of searching"},"target_mime":{"type":"string","description":"e.g., application/pdf, text/csv, application/vnd.openxmlformats-officedocument.wordprocessingml.document"}},"anyOf":[{"required":["q","target_mime"]},{"required":["file_id","target_mime"]}]}).as_object().expect("Schema object").clone()), output_schema: None, annotations: None, icons: None });
        }
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
            "find_and_export" => {
                let q = args.get("q").and_then(|v| v.as_str()).unwrap_or("");
                let file_id_arg = args.get("file_id").and_then(|v| v.as_str());
                let target_mime = args.get("target_mime").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("target_mime is required".to_string()),
                )?;
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let (file_id, base_name, src_mime) = if let Some(fid) = file_id_arg {
                    let (_, meta) = hub
                        .files()
                        .get(fid)
                        .param("fields", "id,name,mimeType")
                        .doit()
                        .await
                        .map_err(|e| ConnectorError::Other(format!("drive get meta: {}", e)))?;
                    (
                        fid.to_string(),
                        meta.name.unwrap_or_else(|| "export".to_string()),
                        meta.mime_type.unwrap_or_default(),
                    )
                } else {
                    if q.is_empty() {
                        return Err(ConnectorError::InvalidParams(
                            "q or file_id is required".into(),
                        ));
                    }
                    let (_, list) = hub
                        .files()
                        .list()
                        .q(q)
                        .page_size(10)
                        .param("fields", "files(id,name,mimeType)")
                        .doit()
                        .await
                        .map_err(|e| ConnectorError::Other(format!("drive list error: {}", e)))?;
                    let mut files = list.files.unwrap_or_default();
                    if files.is_empty() {
                        return Err(ConnectorError::ResourceNotFound);
                    }
                    let f = files.remove(0);
                    (
                        f.id.unwrap_or_default(),
                        f.name.unwrap_or_else(|| "export".into()),
                        f.mime_type.unwrap_or_default(),
                    )
                };
                if !src_mime.starts_with("application/vnd.google-apps.") {
                    return Err(ConnectorError::InvalidParams("Source is not a Google Docs/Sheets/Slides file; use download_file instead.".into()));
                }
                let mut resp = hub
                    .files()
                    .export(&file_id, target_mime)
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("drive export error: {}", e)))?;
                let bytes = hyper::body::to_bytes(resp.body_mut())
                    .await
                    .map_err(|e| ConnectorError::Other(format!("read body: {}", e)))?;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                fn ext_for(m: &str) -> &'static str {
                    match m { "application/pdf" => "pdf", "text/csv" => "csv", "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx", "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx", _ => "bin" }
                }
                let filename = format!("{}.{}", base_name, ext_for(target_mime));
                let v = json!({ "source_file_id": file_id, "source_mime": src_mime, "export_mime_type": target_mime, "filename": filename, "data_base64": b64 });
                structured_result_with_text(&v, None)
            }

            "list_files" => {
                let q = args.get("q").and_then(|v| v.as_str()).unwrap_or("");
                let page_size = args.get("page_size").and_then(|v| v.as_i64()).unwrap_or(25);
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
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;

                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );

                let page_size = page_size.clamp(1, 100) as usize;
                let collected = collect_paginated_with_cursor(
                    desired_limit,
                    100,
                    start_token,
                    |cursor, remaining| {
                        let hub = hub.clone();
                        let q = q.to_string();
                        async move {
                            let per_page = (remaining.min(page_size)).clamp(1, 100) as i32;
                            let mut call = hub
                                .files()
                                .list()
                                .q(&q)
                                .page_size(per_page)
                                .param(
                                    "fields",
                                    "files(id,name,mimeType,modifiedTime,size),nextPageToken",
                                );
                            if let Some(t) = cursor {
                                call = call.param("pageToken", &t);
                            }
                            let (_, file_list) = call.doit().await.map_err(|e| {
                                ConnectorError::Other(format!("drive error: {}", e))
                            })?;

                            let files = file_list
                                .files
                                .unwrap_or_default()
                                .into_iter()
                                .map(|f| {
                                    if concise {
                                        serde_json::json!({
                                            "id": f.id.unwrap_or_default(),
                                            "name": f.name.unwrap_or_default(),
                                            "mime_type": f.mime_type.unwrap_or_default(),
                                            "size": f.size,
                                            "modified_time": f.modified_time.map(|dt| dt.to_rfc3339()),
                                        })
                                    } else {
                                        serde_json::to_value(&f).unwrap_or(json!({}))
                                    }
                                })
                                .collect::<Vec<_>>();

                            Ok::<_, ConnectorError>(Page {
                                items: files,
                                next_cursor: file_list.next_page_token,
                            })
                        }
                    },
                    |f: &serde_json::Value| f.get("id").and_then(|v| v.as_str()).map(str::to_string),
                )
                .await?;

                let v = serde_json::json!({"files": collected.items, "nextPageToken": collected.next_cursor});
                structured_result_with_text(
                    &v,
                    Some(format!(
                        "{} files",
                        v["files"].as_array().map(|a| a.len()).unwrap_or(0)
                    )),
                )
            }
            "get_file" => {
                let file_id = args.get("file_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("file_id is required".to_string()),
                )?;
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;

                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let call = hub
                    .files()
                    .get(file_id)
                    .param("fields", "id,name,mimeType,modifiedTime,size");
                let (_, file) = call
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("drive error: {}", e)))?;
                let concise = !matches!(
                    args.get("response_format").and_then(|v| v.as_str()),
                    Some("detailed")
                );
                if concise {
                    let v = serde_json::json!({
                        "id": file.id.unwrap_or_default(),
                        "name": file.name.unwrap_or_default(),
                        "mime_type": file.mime_type.unwrap_or_default(),
                        "size": file.size,
                        "modified_time": file.modified_time.map(|dt| dt.to_rfc3339()),
                    });
                    structured_result_with_text(&v, None)
                } else {
                    let v = serde_json::to_value(&file)
                        .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                    structured_result_with_text(&v, None)
                }
            }
            "download_file" => {
                let file_id = args.get("file_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("file_id is required".to_string()),
                )?;
                let max_bytes = args.get("max_bytes").and_then(|v| v.as_i64()).unwrap_or(0);
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let (_, meta) = hub
                    .files()
                    .get(file_id)
                    .param("fields", "id,name,mimeType,size")
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("drive get meta: {}", e)))?;
                let (mut resp, _meta) = hub
                    .files()
                    .get(file_id)
                    .param("alt", "media")
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("drive download error: {}", e)))?;
                let bytes = hyper::body::to_bytes(resp.body_mut())
                    .await
                    .map_err(|e| ConnectorError::Other(format!("read body: {}", e)))?;
                if max_bytes > 0 && bytes.len() as i64 > max_bytes {
                    return Err(ConnectorError::InvalidParams(
                        "file too large; use max_bytes or export".to_string(),
                    ));
                }
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let v = json!({ "file_id": file_id, "name": meta.name.unwrap_or_default(), "mime_type": meta.mime_type.unwrap_or_default(), "size": meta.size, "data_base64": b64 });
                structured_result_with_text(&v, None)
            }
            "export_file" => {
                let file_id = args.get("file_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("file_id is required".to_string()),
                )?;
                let mime_type = args.get("mime_type").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("mime_type is required".to_string()),
                )?;
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let (_, meta) = hub
                    .files()
                    .get(file_id)
                    .param("fields", "id,name")
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("drive get meta: {}", e)))?;
                let mut resp = hub
                    .files()
                    .export(file_id, mime_type)
                    .doit()
                    .await
                    .map_err(|e| ConnectorError::Other(format!("drive export error: {}", e)))?;
                let bytes = hyper::body::to_bytes(resp.body_mut())
                    .await
                    .map_err(|e| ConnectorError::Other(format!("read body: {}", e)))?;
                let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                fn ext_for(m: &str) -> &'static str {
                    match m { "application/pdf" => "pdf", "text/csv" => "csv", "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx", _ => "bin" }
                }
                let base = meta.name.unwrap_or_else(|| "export".to_string());
                let filename = format!("{}.{}", base, ext_for(mime_type));
                let v = json!({ "file_id": file_id, "mime_type": mime_type, "filename": filename, "data_base64": b64 });
                structured_result_with_text(&v, None)
            }
            "upload_file" => {
                use base64::Engine as _;
                let name = args.get("name").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("name is required".to_string()),
                )?;
                let mime_type = args.get("mime_type").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("mime_type is required".to_string()),
                )?;
                let data_b64 = args.get("data_base64").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("data_base64 is required".to_string()),
                )?;
                let parents: Vec<String> = args
                    .get("parents")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let mut meta = drive3::api::File {
                    name: Some(name.to_string()),
                    ..Default::default()
                };
                if !parents.is_empty() {
                    meta.parents = Some(parents);
                }
                let data = base64::engine::general_purpose::STANDARD
                    .decode(data_b64)
                    .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(data_b64))
                    .map_err(|e| ConnectorError::InvalidParams(format!("base64 decode: {}", e)))?;
                let cursor = std::io::Cursor::new(data);
                let (_, created) = hub
                    .files()
                    .create(meta)
                    .upload(
                        cursor,
                        mime_type.parse().unwrap_or(
                            "application/octet-stream".parse().expect("Valid MIME type"),
                        ),
                    )
                    .await
                    .map_err(|e| ConnectorError::Other(format!("drive upload error: {}", e)))?;
                let v = serde_json::to_value(&created)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "upload_file_resumable" => {
                use base64::Engine as _;
                let name = args.get("name").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("name is required".to_string()),
                )?;
                let mime_type = args.get("mime_type").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("mime_type is required".to_string()),
                )?;
                let data_b64 = args.get("data_base64").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("data_base64 is required".to_string()),
                )?;
                let parents: Vec<String> = args
                    .get("parents")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let mut meta = drive3::api::File {
                    name: Some(name.to_string()),
                    ..Default::default()
                };
                if !parents.is_empty() {
                    meta.parents = Some(parents);
                }
                let tmp_path = std::env::temp_dir().join(format!(
                    "rzn_drive_upload_{}_{}.bin",
                    name,
                    chrono::Utc::now().timestamp_millis()
                ));
                let data = base64::engine::general_purpose::STANDARD
                    .decode(data_b64)
                    .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(data_b64))
                    .map_err(|e| ConnectorError::InvalidParams(format!("base64 decode: {}", e)))?;
                std::fs::write(&tmp_path, &data)
                    .map_err(|e| ConnectorError::Other(format!("write tmp: {}", e)))?;
                let file = std::fs::File::open(&tmp_path)
                    .map_err(|e| ConnectorError::Other(format!("open tmp: {}", e)))?;
                let (_, created) = hub
                    .files()
                    .create(meta)
                    .upload_resumable(
                        file,
                        mime_type.parse().unwrap_or(
                            "application/octet-stream".parse().expect("Valid MIME type"),
                        ),
                    )
                    .await
                    .map_err(|e| {
                        ConnectorError::Other(format!("drive resumable upload error: {}", e))
                    })?;
                let _ = std::fs::remove_file(&tmp_path);
                let v = serde_json::to_value(&created)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "watch_files" => {
                let address = args.get("address").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("address is required".to_string()),
                )?;
                let id = args.get("id").and_then(|v| v.as_str());
                let token_param = args.get("token").and_then(|v| v.as_str());
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let mut ch = drive3::api::Channel {
                    address: Some(address.to_string()),
                    type_: Some("web_hook".to_string()),
                    ..Default::default()
                };
                if let Some(i) = id {
                    ch.id = Some(i.to_string());
                }
                if let Some(t) = token_param {
                    ch.token = Some(t.to_string());
                }
                let (_, start) =
                    hub.changes()
                        .get_start_page_token()
                        .doit()
                        .await
                        .map_err(|e| {
                            ConnectorError::Other(format!("drive getStartPageToken error: {}", e))
                        })?;
                let page_token = start
                    .start_page_token
                    .ok_or_else(|| ConnectorError::Other("missing startPageToken".to_string()))?;
                let (_, resp) = hub
                    .changes()
                    .watch(ch, &page_token)
                    .doit()
                    .await
                    .map_err(|e| {
                        ConnectorError::Other(format!("drive changes.watch error: {}", e))
                    })?;
                let v = serde_json::to_value(&resp)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "stop_channel" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or(ConnectorError::InvalidParams("id is required".to_string()))?;
                let resource_id = args.get("resource_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("resource_id is required".to_string()),
                )?;
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let ch = drive3::api::Channel {
                    id: Some(id.to_string()),
                    resource_id: Some(resource_id.to_string()),
                    ..Default::default()
                };
                let _ = hub.channels().stop(ch).doit().await.map_err(|e| {
                    ConnectorError::Other(format!("drive channels.stop error: {}", e))
                })?;
                structured_result_with_text(&serde_json::json!({"status":"stopped"}), None)
            }
            "get_start_page_token" => {
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let (_, start) =
                    hub.changes()
                        .get_start_page_token()
                        .doit()
                        .await
                        .map_err(|e| {
                            ConnectorError::Other(format!("drive getStartPageToken error: {}", e))
                        })?;
                let v = serde_json::to_value(&start)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "list_changes" => {
                let page_token = args.get("page_token").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("page_token is required".to_string()),
                )?;
                let page_size = args
                    .get("page_size")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(100);
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let (_, changes) = hub
                    .changes()
                    .list(page_token)
                    .page_size(page_size as i32)
                    .doit()
                    .await
                    .map_err(|e| {
                        ConnectorError::Other(format!("drive changes.list error: {}", e))
                    })?;
                let v = serde_json::to_value(&changes)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "upload_file_from_path" => {
                let file_path = args.get("file_path").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("file_path is required".to_string()),
                )?;
                let mime_type = args
                    .get("mime_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("application/octet-stream");
                let name_override = args.get("name").and_then(|v| v.as_str());
                let parents: Vec<String> = args
                    .get("parents")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());
                let mut meta = drive3::api::File::default();
                let file_name = name_override.map(|s| s.to_string()).unwrap_or_else(|| {
                    std::path::Path::new(file_path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("upload.bin")
                        .to_string()
                });
                meta.name = Some(file_name);
                if !parents.is_empty() {
                    meta.parents = Some(parents);
                }
                let f = std::fs::File::open(std::path::Path::new(file_path))
                    .map_err(|e| ConnectorError::Other(format!("open file: {}", e)))?;
                let (_, created) = hub
                    .files()
                    .create(meta)
                    .upload_resumable(
                        f,
                        mime_type.parse().unwrap_or(
                            "application/octet-stream".parse().expect("Valid MIME type"),
                        ),
                    )
                    .await
                    .map_err(|e| {
                        ConnectorError::Other(format!("drive resumable upload error: {}", e))
                    })?;
                let v = serde_json::to_value(&created)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "watch_file" => {
                let file_id = args.get("file_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("file_id is required".to_string()),
                )?;
                let address = args.get("address").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("address is required".to_string()),
                )?;
                let id = args.get("id").and_then(|v| v.as_str());
                let token_param = args.get("token").and_then(|v| v.as_str());

                let store = FileAuthStore::new_default();
                let auth = store
                    .load(self.name())
                    .or_else(|| store.load("google-common"))
                    .ok_or_else(|| {
                        ConnectorError::Authentication("No tokens stored".to_string())
                    })?;
                let token = auth.get("access_token").cloned().ok_or_else(|| {
                    ConnectorError::Authentication("Missing access_token".to_string())
                })?;
                let client = crate::oauth_client::google_client::new_https_client();
                let hub = drive3::DriveHub::new(client, token.clone());

                let mut ch = drive3::api::Channel {
                    address: Some(address.to_string()),
                    type_: Some("web_hook".to_string()),
                    ..Default::default()
                };
                if let Some(i) = id {
                    ch.id = Some(i.to_string());
                }
                if let Some(t) = token_param {
                    ch.token = Some(t.to_string());
                }
                let (_, resp) = hub.files().watch(ch, file_id).doit().await.map_err(|e| {
                    ConnectorError::Other(format!("drive files.watch error: {}", e))
                })?;
                let v = serde_json::to_value(&resp)
                    .map_err(|e| ConnectorError::Other(format!("serde: {}", e)))?;
                structured_result_with_text(&v, None)
            }
            "auth_start" => {
                let client_id = args.get("client_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("client_id is required".to_string()),
                )?;
                let scopes = args
                    .get("scopes")
                    .and_then(|v| v.as_str())
                    .unwrap_or("https://www.googleapis.com/auth/drive.readonly");
                let start = oauth::google_device_authorize(client_id, scopes).await?;
                let text = serde_json::to_string(&start)?;
                structured_result_with_text(&start, Some(text))
            }
            "auth_poll" => {
                let client_id = args.get("client_id").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("client_id is required".to_string()),
                )?;
                let client_secret = args.get("client_secret").and_then(|v| v.as_str());
                let device_code = args.get("device_code").and_then(|v| v.as_str()).ok_or(
                    ConnectorError::InvalidParams("device_code is required".to_string()),
                )?;
                let tokens =
                    oauth::google_device_poll(client_id, client_secret, device_code).await?;
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
                    let _ = store.save("google-common", &auth);
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
        let store = FileAuthStore::new_default();
        let auth = store
            .load(self.name())
            .or_else(|| store.load("google-common"))
            .unwrap_or_default();
        if auth.contains_key("access_token") || self.auth.contains_key("access_token") {
            return Ok(());
        }
        Err(ConnectorError::Authentication(
            "Google Drive auth not configured".to_string(),
        ))
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "client_id".to_string(),
                    label: "Client ID".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("OAuth client ID for Installed App flow.".to_string()),
                    options: None,
                },
                Field {
                    name: "client_secret".to_string(),
                    label: "Client Secret".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some("OAuth client secret.".to_string()),
                    options: None,
                },
                Field {
                    name: "redirect_uri".to_string(),
                    label: "Redirect URI".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Redirect URI used during auth, e.g. http://localhost:PORT".to_string()),
                    options: None,
                },
                Field {
                    name: "scopes".to_string(),
                    label: "Scopes".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Space-separated scopes, e.g. https://www.googleapis.com/auth/drive.readonly".to_string()),
                    options: None,
                },
            ],
        }
    }
}
