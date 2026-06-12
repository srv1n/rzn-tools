use async_trait::async_trait;
use base64::Engine as _;
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Child;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;

const DEFAULT_WUZAPI_BASE_URL: &str = "http://localhost:8080";
const WUZAPI_STARTUP_WAIT_MS: u64 = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SendMediaType {
    Image,
    Document,
    Audio,
    Video,
    Sticker,
}

#[derive(Debug, Clone, Deserialize)]
struct ConnectArgs {
    /// Events to subscribe to (e.g., Message, ReadReceipt, HistorySync).
    #[serde(default)]
    subscribe: Option<Vec<String>>,
    /// Wait for login verification (best-effort) before returning.
    #[serde(default)]
    immediate: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct SendTextArgs {
    to: String,
    message: String,
    #[serde(default)]
    link_preview: Option<bool>,
    /// Optional custom message id; if omitted, WuzAPI will generate.
    #[serde(default)]
    id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SendMediaArgs {
    to: String,
    file_path: String,
    #[serde(default)]
    caption: Option<String>,
    media_type: SendMediaType,
    /// Optional override MIME type for the data URL prefix.
    #[serde(default)]
    mime_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SendLocationArgs {
    to: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ListContactsArgs {
    #[serde(default)]
    query: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct GetGroupInfoArgs {
    group_jid: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GetHistoryArgs {
    /// WhatsApp JID (e.g. `1234567890@s.whatsapp.net` or `12345-678@g.us`), or the special value
    /// `index` to list available chats.
    chat_jid: String,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct SetHistoryArgs {
    /// Number of messages to keep in history (0 disables history).
    history: i64,
}

#[derive(Default)]
struct WuzapiSidecar {
    child: Option<Child>,
}

pub struct WhatsAppConnector {
    http: reqwest::Client,
    auth: AuthDetails,
    sidecar: tokio::sync::Mutex<WuzapiSidecar>,
}

impl WhatsAppConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let http = reqwest::Client::builder()
            .user_agent("rzn-tools/0.1 whatsapp-connector (wuzapi)")
            .build()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        Ok(Self {
            http,
            auth,
            sidecar: tokio::sync::Mutex::new(WuzapiSidecar::default()),
        })
    }

    fn base_url(&self) -> String {
        let from_auth = self
            .auth
            .get("base_url")
            .or_else(|| self.auth.get("wuzapi_url"))
            .cloned();
        let from_env = std::env::var("WUZAPI_BASE_URL")
            .ok()
            .or_else(|| std::env::var("WUZAPI_URL").ok());

        from_auth
            .or(from_env)
            .unwrap_or_else(|| DEFAULT_WUZAPI_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string()
    }

    fn resolve_token(&self) -> Option<String> {
        if let Some(t) = self
            .auth
            .get("token")
            .or_else(|| self.auth.get("user_token"))
        {
            if !t.trim().is_empty() {
                return Some(t.clone());
            }
        }
        if let Ok(t) = std::env::var("WUZAPI_TOKEN") {
            if !t.trim().is_empty() {
                return Some(t);
            }
        }
        // WuzAPI commonly uses a single "admin token" for API authorization; accept it as a
        // fallback to reduce configuration friction.
        if let Some(t) = self.auth.get("admin_token") {
            if !t.trim().is_empty() {
                return Some(t.clone());
            }
        }
        if let Ok(t) = std::env::var("WUZAPI_ADMIN_TOKEN") {
            if !t.trim().is_empty() {
                return Some(t);
            }
        }
        let store = FileAuthStore::new_default();
        store.load(self.name()).and_then(|m| {
            m.get("token")
                .cloned()
                .or_else(|| m.get("admin_token").cloned())
        })
    }

    fn resolve_admin_token(&self) -> Option<String> {
        if let Some(t) = self.auth.get("admin_token") {
            return Some(t.clone());
        }
        std::env::var("WUZAPI_ADMIN_TOKEN").ok()
    }

    fn resolve_wuzapi_path(&self) -> Option<String> {
        self.auth
            .get("wuzapi_path")
            .cloned()
            .or_else(|| std::env::var("WUZAPI_PATH").ok())
    }

    fn resolve_wuzapi_data_dir(&self) -> Option<PathBuf> {
        self.auth
            .get("data_dir")
            .or_else(|| self.auth.get("wuzapi_data_dir"))
            .map(PathBuf::from)
            .or_else(|| std::env::var("WUZAPI_DATA_DIR").ok().map(PathBuf::from))
    }

    fn resolve_wuzapi_bind(&self) -> (String, u16) {
        let mut address = self
            .auth
            .get("wuzapi_address")
            .cloned()
            .or_else(|| std::env::var("WUZAPI_ADDRESS").ok())
            .unwrap_or_else(|| "127.0.0.1".to_string());

        if address.trim().is_empty() {
            address = "127.0.0.1".to_string();
        }

        let port = self
            .auth
            .get("wuzapi_port")
            .and_then(|s| s.parse::<u16>().ok())
            .or_else(|| {
                std::env::var("WUZAPI_PORT")
                    .ok()
                    .and_then(|s| s.parse::<u16>().ok())
            })
            .unwrap_or(8080);

        (address, port)
    }

    async fn is_healthy(&self) -> bool {
        let url = format!("{}/health", self.base_url());
        match self.http.get(url).send().await {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    }

    async fn ensure_running(&self) -> Result<(), ConnectorError> {
        if self.is_healthy().await {
            return Ok(());
        }

        let Some(wuzapi_path) = self.resolve_wuzapi_path() else {
            return Err(ConnectorError::Other(
                "WuzAPI is not reachable and no wuzapi_path is configured. Start WuzAPI separately or set whatsapp.wuzapi_path + whatsapp.base_url.".to_string(),
            ));
        };

        {
            // If another task already started the sidecar, just wait for it.
            let sidecar = self.sidecar.lock().await;
            if sidecar.child.is_some() {
                drop(sidecar);
                return self.wait_for_health().await;
            }
        }

        let (address, port) = self.resolve_wuzapi_bind();
        let data_dir = self
            .resolve_wuzapi_data_dir()
            .unwrap_or_else(default_wuzapi_data_dir);
        let admin_token = self.resolve_admin_token();

        let mut cmd = tokio::process::Command::new(&wuzapi_path);
        cmd.arg("-address")
            .arg(address)
            .arg("-port")
            .arg(port.to_string())
            .arg("-datadir")
            .arg(data_dir);

        if let Some(token) = admin_token {
            cmd.arg("-admintoken").arg(token);
        }

        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());

        let child = cmd.spawn().map_err(|e| {
            ConnectorError::Other(format!("Failed to start WuzAPI at {wuzapi_path}: {e}"))
        })?;

        let mut sidecar = self.sidecar.lock().await;
        sidecar.child = Some(child);
        drop(sidecar);

        self.wait_for_health().await
    }

    async fn wait_for_health(&self) -> Result<(), ConnectorError> {
        use tokio::time::{sleep, Duration, Instant};
        let start = Instant::now();
        let mut delay_ms = 150u64;

        while start.elapsed() < Duration::from_millis(WUZAPI_STARTUP_WAIT_MS) {
            if self.is_healthy().await {
                return Ok(());
            }
            sleep(Duration::from_millis(delay_ms)).await;
            delay_ms = (delay_ms as f64 * 1.6).min(1200.0) as u64;
        }

        Err(ConnectorError::Other(
            "Timed out waiting for WuzAPI /health to become ready".to_string(),
        ))
    }

    async fn api_get(
        &self,
        path: &str,
        query: Option<&[(&str, String)]>,
    ) -> Result<Value, ConnectorError> {
        self.ensure_running().await?;
        let token = self.resolve_token().ok_or_else(|| {
            ConnectorError::Authentication("WuzAPI user token not configured".to_string())
        })?;

        let url = format!("{}/{}", self.base_url(), path.trim_start_matches('/'));
        let mut req = self
            .http
            .get(&url)
            .header("Authorization", token.clone())
            .header("Token", token);
        if let Some(q) = query {
            req = req.query(q);
        }
        let resp = req.send().await.map_err(ConnectorError::HttpRequest)?;
        parse_wuzapi_json(resp).await
    }

    async fn api_post(&self, path: &str, body: Value) -> Result<Value, ConnectorError> {
        self.ensure_running().await?;
        let token = self.resolve_token().ok_or_else(|| {
            ConnectorError::Authentication("WuzAPI user token not configured".to_string())
        })?;

        let url = format!("{}/{}", self.base_url(), path.trim_start_matches('/'));
        let resp = self
            .http
            .post(&url)
            .header("Authorization", token.clone())
            .header("Token", token)
            .json(&body)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;

        parse_wuzapi_json(resp).await
    }

    async fn api_post_no_body(&self, path: &str) -> Result<Value, ConnectorError> {
        self.ensure_running().await?;
        let token = self.resolve_token().ok_or_else(|| {
            ConnectorError::Authentication("WuzAPI user token not configured".to_string())
        })?;
        let url = format!("{}/{}", self.base_url(), path.trim_start_matches('/'));
        let resp = self
            .http
            .post(&url)
            .header("Authorization", token.clone())
            .header("Token", token)
            .send()
            .await
            .map_err(ConnectorError::HttpRequest)?;
        parse_wuzapi_json(resp).await
    }

    async fn stop_sidecar(&self) -> Result<Value, ConnectorError> {
        let mut sidecar = self.sidecar.lock().await;
        if let Some(mut child) = sidecar.child.take() {
            let _ = child.kill().await;
            return Ok(json!({ "stopped": true }));
        }
        Ok(json!({ "stopped": false, "message": "No managed WuzAPI process to stop" }))
    }
}

fn default_wuzapi_data_dir() -> PathBuf {
    let dir = crate::auth_store::config_dir().join("wuzapi");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

async fn parse_wuzapi_json(resp: reqwest::Response) -> Result<Value, ConnectorError> {
    let status = resp.status();
    let v: Value = resp.json().await.map_err(ConnectorError::HttpRequest)?;

    let success = v.get("success").and_then(|b| b.as_bool()).unwrap_or(false);
    if status.is_success() && success {
        return Ok(v);
    }

    let err = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("WuzAPI request failed");
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(ConnectorError::Authentication(err.to_string()));
    }
    Err(ConnectorError::Other(format!(
        "WuzAPI error {}: {}",
        status.as_u16(),
        err
    )))
}

fn guess_mime(media_type: &SendMediaType, path: &Path, override_mime: Option<&str>) -> String {
    if let Some(m) = override_mime {
        if !m.trim().is_empty() {
            return m.trim().to_string();
        }
    }

    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match media_type {
        SendMediaType::Image => match ext.as_str() {
            "png" => "image/png",
            "webp" => "image/webp",
            _ => "image/jpeg",
        },
        SendMediaType::Video => match ext.as_str() {
            "3gp" | "3gpp" => "video/3gpp",
            _ => "video/mp4",
        },
        SendMediaType::Audio => match ext.as_str() {
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            _ => "audio/ogg",
        },
        SendMediaType::Sticker => match ext.as_str() {
            "mp4" => "video/mp4",
            _ => "image/webp",
        },
        SendMediaType::Document => "application/octet-stream",
    }
    .to_string()
}

async fn file_to_data_url(
    file_path: &str,
    media_type: &SendMediaType,
    mime_type: Option<&str>,
) -> Result<String, ConnectorError> {
    let path = Path::new(file_path);
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| ConnectorError::Other(format!("Failed to read file {file_path}: {e}")))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mime = guess_mime(media_type, path, mime_type);
    Ok(format!("data:{mime};base64,{b64}"))
}

#[async_trait]
impl Connector for WhatsAppConnector {
    fn name(&self) -> &'static str {
        "whatsapp"
    }

    fn description(&self) -> &'static str {
        "WhatsApp via local WuzAPI (whatsmeow). Unofficial client; ToS risk."
    }

    fn display_name(&self) -> &'static str {
        "WhatsApp"
    }

    fn icon(&self) -> &'static str {
        "message-circle"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["communication"]
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
                "Requires WuzAPI (https://github.com/asternic/wuzapi). Configure `token` and optional `base_url` and `wuzapi_path`.\nWarning: This uses an unofficial WhatsApp client (whatsmeow) and may violate WhatsApp ToS."
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
                name: Cow::Borrowed("health"),
                title: None,
                description: Some(Cow::Borrowed("Check if WuzAPI is reachable (/health).")),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("connect"),
                title: None,
                description: Some(Cow::Borrowed("Connect to WhatsApp. If not logged in, returns QR code data URL to scan.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "subscribe":{"type":"array","items":{"type":"string"}},
                        "immediate":{"type":"boolean"}
                    }
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("status"),
                title: None,
                description: Some(Cow::Borrowed("Get session status (connected/logged_in).")),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("disconnect"),
                title: None,
                description: Some(Cow::Borrowed("Disconnect websocket without logging out (keeps session).")),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("logout"),
                title: None,
                description: Some(Cow::Borrowed("Logout and clear session (QR required next time).")),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("stop_sidecar"),
                title: None,
                description: Some(Cow::Borrowed("Stop the managed WuzAPI process (only if rzn-tools started it).")),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("send_text"),
                title: None,
                description: Some(Cow::Borrowed("Send a text message to a phone number or JID (user or group).")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "to":{"type":"string"},
                        "message":{"type":"string"},
                        "link_preview":{"type":"boolean"},
                        "id":{"type":"string"}
                    },
                    "required":["to","message"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("send_media"),
                title: None,
                description: Some(Cow::Borrowed("Send media by reading a local file and encoding as a data URL.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "to":{"type":"string"},
                        "file_path":{"type":"string"},
                        "caption":{"type":"string"},
                        "media_type":{"type":"string","enum":["image","document","audio","video","sticker"]},
                        "mime_type":{"type":"string"}
                    },
                    "required":["to","file_path","media_type"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("send_location"),
                title: None,
                description: Some(Cow::Borrowed("Send a location pin.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "to":{"type":"string"},
                        "latitude":{"type":"number"},
                        "longitude":{"type":"number"},
                        "name":{"type":"string"}
                    },
                    "required":["to","latitude","longitude"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_contacts"),
                title: None,
                description: Some(Cow::Borrowed("List synced contacts (may be large).")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{"query":{"type":"string"}}
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_groups"),
                title: None,
                description: Some(Cow::Borrowed("List joined groups.")),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_group_info"),
                title: None,
                description: Some(Cow::Borrowed("Get group details by group JID.")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{"group_jid":{"type":"string"}},
                    "required":["group_jid"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_messages"),
                title: None,
                description: Some(Cow::Borrowed("Get chat history from WuzAPI local message_history (requires history enabled).")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{
                        "chat_jid":{"type":"string"},
                        "limit":{"type":"integer","minimum":1,"maximum":500}
                    },
                    "required":["chat_jid"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("set_history"),
                title: None,
                description: Some(Cow::Borrowed("Enable/disable message history capture in WuzAPI (0 disables).")),
                input_schema: Arc::new(json!({
                    "type":"object",
                    "properties":{"history":{"type":"integer","minimum":0,"maximum":100000}},
                    "required":["history"]
                }).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("request_history_sync"),
                title: None,
                description: Some(Cow::Borrowed("Request a history sync (best-effort, WuzAPI-dependent).")),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().unwrap().clone()),
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
        let name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();
        let args_map = serde_json::Map::from_iter(args);

        match name {
            "health" => {
                self.ensure_running().await?;
                structured_result_with_text(
                    &json!({ "ok": true, "base_url": self.base_url() }),
                    None,
                )
            }
            "connect" => {
                let input: ConnectArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let subscribe = input
                    .subscribe
                    .unwrap_or_else(|| vec!["Message".to_string()]);
                let immediate = input.immediate.unwrap_or(true);
                let connect_body = json!({
                    "Subscribe": subscribe,
                    "Immediate": immediate,
                });
                let connect_resp = self.api_post("/session/connect", connect_body).await?;
                let status_resp = self.api_get("/session/status", None).await?;

                let logged_in = status_resp
                    .get("data")
                    .and_then(|d| d.get("LoggedIn"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let connected = status_resp
                    .get("data")
                    .and_then(|d| d.get("Connected"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if logged_in {
                    let out = json!({
                        "status": "connected",
                        "connected": connected,
                        "logged_in": logged_in,
                        "connect_response": connect_resp,
                        "session_status": status_resp,
                    });
                    structured_result_with_text(&out, None)
                } else {
                    let qr = self.api_get("/session/qr", None).await?;
                    let qr_code = qr
                        .get("data")
                        .and_then(|d| d.get("QRCode"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let out = json!({
                        "status": "qr_required",
                        "connected": connected,
                        "logged_in": logged_in,
                        "qr_code": qr_code,
                        "connect_response": connect_resp,
                        "session_status": status_resp,
                    });
                    structured_result_with_text(&out, None)
                }
            }
            "status" => {
                let v = self.api_get("/session/status", None).await?;
                structured_result_with_text(&v, None)
            }
            "disconnect" => {
                let v = self.api_post_no_body("/session/disconnect").await?;
                structured_result_with_text(&v, None)
            }
            "logout" => {
                let v = self.api_post_no_body("/session/logout").await?;
                structured_result_with_text(&v, None)
            }
            "stop_sidecar" => {
                let v = self.stop_sidecar().await?;
                structured_result_with_text(&v, None)
            }
            "send_text" => {
                let input: SendTextArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let body = json!({
                    "Phone": input.to,
                    "Body": input.message,
                    "LinkPreview": input.link_preview.unwrap_or(false),
                    "Id": input.id.unwrap_or_default(),
                });
                let v = self.api_post("/chat/send/text", body).await?;
                structured_result_with_text(&v, None)
            }
            "send_media" => {
                let input: SendMediaArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let data_url = file_to_data_url(
                    &input.file_path,
                    &input.media_type,
                    input.mime_type.as_deref(),
                )
                .await?;

                let endpoint_and_field = match input.media_type {
                    SendMediaType::Image => ("/chat/send/image", "Image"),
                    SendMediaType::Document => ("/chat/send/document", "Document"),
                    SendMediaType::Audio => ("/chat/send/audio", "Audio"),
                    SendMediaType::Video => ("/chat/send/video", "Video"),
                    SendMediaType::Sticker => ("/chat/send/sticker", "Sticker"),
                };

                let mut body = json!({
                    "Phone": input.to,
                    endpoint_and_field.1: data_url,
                });

                if matches!(
                    input.media_type,
                    SendMediaType::Image | SendMediaType::Video
                ) {
                    if let Some(c) = input.caption {
                        body["Caption"] = Value::String(c);
                    }
                }

                if matches!(input.media_type, SendMediaType::Document) {
                    let filename = Path::new(&input.file_path)
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("file")
                        .to_string();
                    body["FileName"] = Value::String(filename);
                }

                let v = self.api_post(endpoint_and_field.0, body).await?;
                structured_result_with_text(&v, None)
            }
            "send_location" => {
                let input: SendLocationArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut body = json!({
                    "Phone": input.to,
                    "Latitude": input.latitude,
                    "Longitude": input.longitude,
                });
                if let Some(n) = input.name {
                    body["Name"] = Value::String(n);
                }
                let v = self.api_post("/chat/send/location", body).await?;
                structured_result_with_text(&v, None)
            }
            "list_contacts" => {
                let input: ListContactsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self.api_get("/user/contacts", None).await?;
                if let Some(q) = input.query {
                    let q = q.to_ascii_lowercase();
                    // Contacts is usually a map keyed by JID; filter best-effort.
                    let filtered = match v.get("data") {
                        Some(Value::Object(map)) => {
                            let kept = map
                                .iter()
                                .filter(|(jid, info)| {
                                    jid.to_ascii_lowercase().contains(&q)
                                        || info.to_string().to_ascii_lowercase().contains(&q)
                                })
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect::<serde_json::Map<String, Value>>();
                            json!({ "code": v.get("code").cloned().unwrap_or(json!(200)), "success": true, "data": kept })
                        }
                        _ => v.clone(),
                    };
                    structured_result_with_text(&filtered, None)
                } else {
                    structured_result_with_text(&v, None)
                }
            }
            "list_groups" => {
                let v = self.api_get("/group/list", None).await?;
                structured_result_with_text(&v, None)
            }
            "get_group_info" => {
                let input: GetGroupInfoArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let params = [("groupJID", input.group_jid)];
                let v = self.api_get("/group/info", Some(&params)).await?;
                structured_result_with_text(&v, None)
            }
            "get_messages" => {
                let input: GetHistoryArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let mut params = vec![("chat_jid", input.chat_jid)];
                if let Some(limit) = input.limit {
                    params.push(("limit", limit.to_string()));
                }
                let v = self.api_get("/chat/history", Some(&params)).await?;
                structured_result_with_text(&v, None)
            }
            "set_history" => {
                let input: SetHistoryArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let v = self
                    .api_post("/session/history", json!({ "history": input.history }))
                    .await?;
                structured_result_with_text(&v, None)
            }
            "request_history_sync" => {
                let v = self.api_get("/session/history", None).await?;
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
        Err(ConnectorError::InvalidParams(
            "Prompt not found".to_string(),
        ))
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        Ok(self.auth.clone())
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.auth = details.clone();
        if !self.auth.is_empty() {
            let store = FileAuthStore::new_default();
            let _ = store.save(self.name(), &details);
        }
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let _ = self.api_get("/session/status", None).await?;
        Ok(())
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "token".to_string(),
                    label: "WuzAPI Token".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Token used to authorize requests to WuzAPI. Required for API calls. If you only have WUZAPI_ADMIN_TOKEN, you can use it here as well."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "base_url".to_string(),
                    label: "WuzAPI Base URL".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Base URL for WuzAPI (default http://localhost:8080).".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "wuzapi_path".to_string(),
                    label: "WuzAPI Binary Path (optional)".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "If set, rzn-tools can attempt to start WuzAPI automatically when not reachable."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "wuzapi_address".to_string(),
                    label: "WuzAPI Bind Address (optional)".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some("Bind address used when starting WuzAPI (default 127.0.0.1).".to_string()),
                    options: None,
                },
                Field {
                    name: "wuzapi_port".to_string(),
                    label: "WuzAPI Port (optional)".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                    description: Some("Port used when starting WuzAPI (default 8080).".to_string()),
                    options: None,
                },
                Field {
                    name: "data_dir".to_string(),
                    label: "WuzAPI Data Directory (optional)".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Data directory passed to WuzAPI -datadir (stores sqlite db).".to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "admin_token".to_string(),
                    label: "WuzAPI Admin Token (optional)".to_string(),
                    field_type: FieldType::Secret,
                    required: false,
                    description: Some(
                        "Admin token used when starting WuzAPI (sets -admintoken). If `token` is not set, this value is also used to authorize API calls."
                            .to_string(),
                    ),
                    options: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_guessing_has_stable_defaults() {
        let p = Path::new("photo.jpg");
        assert_eq!(
            guess_mime(&SendMediaType::Image, p, None),
            "image/jpeg".to_string()
        );
        let p = Path::new("photo.png");
        assert_eq!(
            guess_mime(&SendMediaType::Image, p, None),
            "image/png".to_string()
        );
        let p = Path::new("clip.mp4");
        assert_eq!(
            guess_mime(&SendMediaType::Video, p, None),
            "video/mp4".to_string()
        );
    }
}
