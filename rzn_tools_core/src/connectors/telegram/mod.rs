use async_trait::async_trait;
use grammers_client::types::LoginToken;
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::defs::{PeerAuth, PeerId, PeerKind, PeerRef};
#[allow(deprecated)]
use grammers_session::storages::TlSession;
use rmcp::model::*;
use serde::Deserialize;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::auth_store::{AuthStore, FileAuthStore};
use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;

const DEFAULT_SESSION_FILE_NAME: &str = "telegram.session";
const MAX_DIALOGS: u32 = 500;
const MAX_MESSAGES: u32 = 500;

#[derive(Debug, Clone, Deserialize)]
struct StartLoginArgs {
    phone: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CompleteLoginArgs {
    code: String,
    #[serde(default)]
    password: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ListDialogsArgs {
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
struct PeerRefArgs {
    /// Bot API dialog id (users: positive, group chats: negative, channels: -100...).
    id: i64,
    /// Access hash from Telegram (required for non-contacts/non-bot sessions).
    access_hash: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct ResolveUsernameArgs {
    username: String,
}

#[derive(Debug, Clone, Deserialize)]
struct SendMessageArgs {
    peer_ref: PeerRefArgs,
    message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GetMessagesArgs {
    peer_ref: PeerRefArgs,
    #[serde(default)]
    limit: Option<u32>,
    /// Upper-bound message id (pagination).
    #[serde(default)]
    before_id: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
struct SearchMessagesArgs {
    peer_ref: PeerRefArgs,
    query: String,
    #[serde(default)]
    limit: Option<u32>,
    #[serde(default)]
    before_id: Option<i32>,
}

struct TelegramRuntime {
    client: Client,
    runner: tokio::task::JoinHandle<()>,
    pending_login: Option<LoginToken>,
}

pub struct TelegramConnector {
    auth: AuthDetails,
    runtime: tokio::sync::Mutex<Option<TelegramRuntime>>,
}

impl TelegramConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        Ok(Self {
            auth,
            runtime: tokio::sync::Mutex::new(None),
        })
    }

    fn resolve_api_id(&self) -> Result<i32, ConnectorError> {
        let from_auth = self
            .auth
            .get("api_id")
            .or_else(|| self.auth.get("tg_api_id"))
            .cloned();
        let from_env = std::env::var("TG_ID").ok();
        let Some(raw) = from_auth.or(from_env) else {
            return Err(ConnectorError::Authentication(
                "Missing Telegram api_id (set telegram.api_id or TG_ID)".to_string(),
            ));
        };
        raw.parse::<i32>().map_err(|_| {
            ConnectorError::Authentication("Telegram api_id must be a number".to_string())
        })
    }

    fn resolve_api_hash(&self) -> Result<String, ConnectorError> {
        let from_auth = self
            .auth
            .get("api_hash")
            .or_else(|| self.auth.get("tg_api_hash"))
            .cloned();
        let from_env = std::env::var("TG_HASH").ok();
        let Some(raw) = from_auth.or(from_env) else {
            return Err(ConnectorError::Authentication(
                "Missing Telegram api_hash (set telegram.api_hash or TG_HASH)".to_string(),
            ));
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ConnectorError::Authentication(
                "Telegram api_hash is empty".to_string(),
            ));
        }
        Ok(trimmed.to_string())
    }

    fn resolve_session_file(&self) -> PathBuf {
        if let Some(p) = self
            .auth
            .get("session_file")
            .or_else(|| self.auth.get("session_path"))
        {
            return PathBuf::from(p);
        }
        if let Ok(p) = std::env::var("TG_SESSION_FILE") {
            if !p.trim().is_empty() {
                return PathBuf::from(p);
            }
        }

        let dir = crate::auth_store::config_dir();
        let _ = std::fs::create_dir_all(&dir);
        dir.join(DEFAULT_SESSION_FILE_NAME)
    }

    async fn reset_runtime(&self) {
        let mut guard = self.runtime.lock().await;
        if let Some(rt) = guard.take() {
            rt.runner.abort();
        }
    }

    async fn ensure_client(&self) -> Result<Client, ConnectorError> {
        {
            let guard = self.runtime.lock().await;
            if let Some(rt) = guard.as_ref() {
                return Ok(rt.client.clone());
            }
        }

        let api_id = self.resolve_api_id()?;
        let session_file = self.resolve_session_file();

        let session_file_for_open = session_file.clone();
        let session = tokio::task::spawn_blocking(move || {
            #[allow(deprecated)]
            TlSession::load_file_or_create(session_file_for_open)
        })
        .await
        .map_err(|e| {
            ConnectorError::Other(format!("Failed to join Telegram session open task: {e}"))
        })?
        .map_err(|e| ConnectorError::Other(format!("Failed to open Telegram session: {e}")))?;
        let session = Arc::new(session);

        let pool = SenderPool::new(Arc::clone(&session), api_id);
        let client = Client::new(&pool);
        let SenderPool { runner, .. } = pool;
        let runner_handle = tokio::spawn(runner.run());

        let mut guard = self.runtime.lock().await;
        *guard = Some(TelegramRuntime {
            client: client.clone(),
            runner: runner_handle,
            pending_login: None,
        });

        Ok(client)
    }

    async fn ensure_authorized(&self, client: &Client) -> Result<(), ConnectorError> {
        let ok = client
            .is_authorized()
            .await
            .map_err(|e| ConnectorError::Other(format!("Telegram auth check failed: {e}")))?;
        if ok {
            Ok(())
        } else {
            Err(ConnectorError::Authentication(
                "Telegram session not authorized. Use telegram/start_login then telegram/complete_login."
                    .to_string(),
            ))
        }
    }

    fn to_peer_ref(args: PeerRefArgs) -> Result<PeerRef, ConnectorError> {
        let id = peer_id_from_bot_api_dialog_id(args.id)?;
        Ok(PeerRef {
            id,
            auth: PeerAuth::from_hash(args.access_hash),
        })
    }
}

fn peer_id_from_bot_api_dialog_id(dialog_id: i64) -> Result<PeerId, ConnectorError> {
    // These bounds mirror the checks in grammers-session to avoid panics on invalid input.
    const MAX_USER_ID: i64 = 0xffffffffff; // 1099511627775
    const MAX_CHAT_ID: i64 = 999_999_999_999;
    const CHANNEL_BOUNDARY: i64 = -1_000_000_000_001; // -1000000000000 - 1
    const CHANNEL_OFFSET: i64 = 1_000_000_000_000;
    const MAX_CHANNEL_ID_1: i64 = 997_852_516_352;
    const MIN_CHANNEL_ID_2: i64 = 1_002_147_483_649;
    const MAX_CHANNEL_ID_2: i64 = 3_000_000_000_000;

    if dialog_id == 0 {
        return Err(ConnectorError::InvalidParams(
            "Telegram dialog id must be non-zero".to_string(),
        ));
    }

    if dialog_id > 0 {
        if dialog_id > MAX_USER_ID {
            return Err(ConnectorError::InvalidParams(format!(
                "Telegram user id out of range: {}",
                dialog_id
            )));
        }
        return Ok(PeerId::user(dialog_id));
    }

    if dialog_id <= CHANNEL_BOUNDARY {
        let bare = -dialog_id - CHANNEL_OFFSET;
        let ok = (1..=MAX_CHANNEL_ID_1).contains(&bare)
            || (MIN_CHANNEL_ID_2..=MAX_CHANNEL_ID_2).contains(&bare);
        if !ok {
            return Err(ConnectorError::InvalidParams(format!(
                "Telegram channel id out of range: {}",
                dialog_id
            )));
        }
        return Ok(PeerId::channel(bare));
    }

    let bare = -dialog_id;
    if bare > MAX_CHAT_ID {
        return Err(ConnectorError::InvalidParams(format!(
            "Telegram chat id out of range: {}",
            dialog_id
        )));
    }
    Ok(PeerId::chat(bare))
}

fn peer_kind_string(id: PeerId) -> &'static str {
    match id.kind() {
        PeerKind::User | PeerKind::UserSelf => "user",
        PeerKind::Chat => "chat",
        PeerKind::Channel => "channel",
    }
}

#[async_trait]
impl Connector for TelegramConnector {
    fn name(&self) -> &'static str {
        "telegram"
    }

    fn description(&self) -> &'static str {
        "Telegram via MTProto (grammers). Requires a local session file."
    }

    fn display_name(&self) -> &'static str {
        "Telegram"
    }

    fn icon(&self) -> &'static str {
        "send"
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
                "Configure Telegram api_id + api_hash (from https://my.telegram.org) and then login using telegram/start_login + telegram/complete_login to create a session file.\nThis connector accesses personal chats; use carefully."
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
                name: Cow::Borrowed("status"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Return whether the current session is authorized.",
                )),
                input_schema: Arc::new(
                    json!({"type":"object","properties":{}})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("start_login"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Send a login code to the given phone number (international format).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{"phone":{"type":"string"}},
                        "required":["phone"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("complete_login"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Complete login with the received code (and optional 2FA password).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "code":{"type":"string"},
                            "password":{"type":"string"}
                        },
                        "required":["code"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("resolve_username"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Resolve a @username to a peer reference (id + access_hash).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{"username":{"type":"string"}},
                        "required":["username"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_dialogs"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List recent dialogs (chats). Returns peer refs usable by other tools.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "limit":{"type":"integer","minimum":1,"maximum":500}
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_messages"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get recent messages from a dialog (by peer_ref).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "peer_ref":{
                                "type":"object",
                                "properties":{
                                    "id":{"type":"integer"},
                                    "access_hash":{"type":"integer"}
                                },
                                "required":["id","access_hash"]
                            },
                            "limit":{"type":"integer","minimum":1,"maximum":500},
                            "before_id":{"type":"integer","minimum":1}
                        },
                        "required":["peer_ref"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_messages"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search messages within a dialog (by peer_ref).",
                )),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "peer_ref":{
                                "type":"object",
                                "properties":{
                                    "id":{"type":"integer"},
                                    "access_hash":{"type":"integer"}
                                },
                                "required":["id","access_hash"]
                            },
                            "query":{"type":"string"},
                            "limit":{"type":"integer","minimum":1,"maximum":500},
                            "before_id":{"type":"integer","minimum":1}
                        },
                        "required":["peer_ref","query"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("send_message"),
                title: None,
                description: Some(Cow::Borrowed("Send a message to a dialog (by peer_ref).")),
                input_schema: Arc::new(
                    json!({
                        "type":"object",
                        "properties":{
                            "peer_ref":{
                                "type":"object",
                                "properties":{
                                    "id":{"type":"integer"},
                                    "access_hash":{"type":"integer"}
                                },
                                "required":["id","access_hash"]
                            },
                            "message":{"type":"string"}
                        },
                        "required":["peer_ref","message"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
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
        let name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();
        let args_map = serde_json::Map::from_iter(args);

        match name {
            "status" => {
                let client = self.ensure_client().await?;
                let authorized = client.is_authorized().await.map_err(|e| {
                    ConnectorError::Other(format!("Telegram auth check failed: {e}"))
                })?;

                let me = if authorized {
                    Some(
                        client
                            .get_me()
                            .await
                            .map(|u| {
                                let id = PeerId::user(u.bare_id()).bot_api_dialog_id();
                                json!({
                                    "id": id,
                                    "username": u.username(),
                                    "name": u.full_name(),
                                })
                            })
                            .unwrap_or(json!(null)),
                    )
                } else {
                    None
                };

                let out = json!({
                    "authorized": authorized,
                    "session_file": self.resolve_session_file().to_string_lossy(),
                    "me": me,
                });
                structured_result_with_text(&out, None)
            }
            "start_login" => {
                let input: StartLoginArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let api_hash = self.resolve_api_hash()?;
                let client = self.ensure_client().await?;

                if client.is_authorized().await.map_err(|e| {
                    ConnectorError::Other(format!("Telegram auth check failed: {e}"))
                })? {
                    let out = json!({ "status": "already_authorized" });
                    return structured_result_with_text(&out, None);
                }

                let token = client
                    .request_login_code(&input.phone, &api_hash)
                    .await
                    .map_err(|e| {
                        ConnectorError::Other(format!("Failed to request login code: {e}"))
                    })?;

                let mut guard = self.runtime.lock().await;
                let Some(rt) = guard.as_mut() else {
                    return Err(ConnectorError::Other(
                        "Internal error: telegram runtime not initialized".to_string(),
                    ));
                };
                rt.pending_login = Some(token);

                let out = json!({ "status": "code_sent" });
                structured_result_with_text(&out, None)
            }
            "complete_login" => {
                let input: CompleteLoginArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let client = self.ensure_client().await?;

                let token = {
                    let mut guard = self.runtime.lock().await;
                    let Some(rt) = guard.as_mut() else {
                        return Err(ConnectorError::Other(
                            "Internal error: telegram runtime not initialized".to_string(),
                        ));
                    };
                    rt.pending_login.take().ok_or_else(|| {
                        ConnectorError::InvalidParams(
                            "No pending login. Call telegram/start_login first.".to_string(),
                        )
                    })?
                };

                let signed_in = client.sign_in(&token, &input.code).await;
                match signed_in {
                    Ok(_) => {
                        let out = json!({ "status": "authorized" });
                        structured_result_with_text(&out, None)
                    }
                    Err(SignInError::PasswordRequired(password_token)) => {
                        let hint = password_token.hint().unwrap_or("None").to_string();
                        let Some(password) = input.password else {
                            // Put token back to allow retry.
                            let mut guard = self.runtime.lock().await;
                            if let Some(rt) = guard.as_mut() {
                                rt.pending_login = Some(token);
                            }
                            return structured_result_with_text(
                                &json!({ "status": "password_required", "hint": hint }),
                                None,
                            );
                        };
                        client
                            .check_password(password_token, password.trim())
                            .await
                            .map_err(|e| {
                                ConnectorError::Authentication(format!("Invalid password: {e}"))
                            })?;
                        let out = json!({ "status": "authorized" });
                        structured_result_with_text(&out, None)
                    }
                    Err(SignInError::InvalidCode) => Err(ConnectorError::Authentication(
                        "Invalid Telegram code".to_string(),
                    )),
                    Err(SignInError::SignUpRequired { .. }) => Err(ConnectorError::Authentication(
                        "Telegram sign-up required via official client".to_string(),
                    )),
                    Err(SignInError::InvalidPassword) => Err(ConnectorError::Authentication(
                        "Invalid Telegram 2FA password".to_string(),
                    )),
                    Err(SignInError::Other(e)) => Err(ConnectorError::Other(format!(
                        "Telegram sign-in failed: {e}"
                    ))),
                }
            }
            "resolve_username" => {
                let input: ResolveUsernameArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;
                let client = self.ensure_client().await?;
                self.ensure_authorized(&client).await?;

                let uname = input.username.trim().trim_start_matches('@');
                if uname.is_empty() {
                    return Err(ConnectorError::InvalidParams(
                        "username is empty".to_string(),
                    ));
                }

                let peer = client
                    .resolve_username(uname)
                    .await
                    .map_err(|e| ConnectorError::Other(format!("Failed to resolve username: {e}")))?
                    .ok_or_else(|| ConnectorError::Other("Username not found".to_string()))?;

                let peer_ref = PeerRef::from(&peer);

                let out = json!({
                    "peer": {
                        "id": peer.id().bot_api_dialog_id(),
                        "kind": peer_kind_string(peer.id()),
                        "name": peer.name(),
                        "username": peer.username(),
                    },
                    "peer_ref": {
                        "id": peer_ref.id.bot_api_dialog_id(),
                        "access_hash": peer_ref.auth.hash()
                    }
                });
                structured_result_with_text(&out, None)
            }
            "list_dialogs" => {
                let input: ListDialogsArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let client = self.ensure_client().await?;
                self.ensure_authorized(&client).await?;

                let desired = input.limit.unwrap_or(50).clamp(1, MAX_DIALOGS) as usize;

                let mut dialogs = client.iter_dialogs();
                let mut out = Vec::with_capacity(desired);

                while out.len() < desired {
                    let Some(dialog) = dialogs.next().await.map_err(|e| {
                        ConnectorError::Other(format!("Failed to list dialogs: {e}"))
                    })?
                    else {
                        break;
                    };
                    let peer = dialog.peer();
                    let peer_ref = PeerRef::from(peer);
                    let peer_ref_obj = json!({
                        "id": peer_ref.id.bot_api_dialog_id(),
                        "access_hash": peer_ref.auth.hash()
                    });

                    out.push(json!({
                        "id": peer.id().bot_api_dialog_id(),
                        "kind": peer_kind_string(peer.id()),
                        "name": peer.name(),
                        "username": peer.username(),
                        "peer_ref": peer_ref_obj,
                    }));
                }

                structured_result_with_text(&json!({ "dialogs": out }), None)
            }
            "get_messages" => {
                let input: GetMessagesArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let client = self.ensure_client().await?;
                self.ensure_authorized(&client).await?;

                let peer_ref = Self::to_peer_ref(input.peer_ref)?;
                let desired = input.limit.unwrap_or(50).clamp(1, MAX_MESSAGES) as usize;

                let mut iter = client.iter_messages(peer_ref);
                if let Some(before_id) = input.before_id {
                    iter = iter.offset_id(before_id);
                }

                let mut messages = Vec::with_capacity(desired);
                let mut oldest_id: Option<i32> = None;

                while messages.len() < desired {
                    let Some(msg) = iter.next().await.map_err(|e| {
                        ConnectorError::Other(format!("Failed to fetch messages: {e}"))
                    })?
                    else {
                        break;
                    };

                    let sender_id = msg.sender().map(|p| p.id().bot_api_dialog_id());
                    let peer_id = msg.peer_id().bot_api_dialog_id();
                    let msg_id = msg.id();
                    oldest_id = Some(oldest_id.map_or(msg_id, |cur| cur.min(msg_id)));

                    messages.push(json!({
                        "id": msg_id,
                        "date": msg.date().to_rfc3339(),
                        "outgoing": msg.outgoing(),
                        "peer_id": peer_id,
                        "sender_id": sender_id,
                        "reply_to": msg.reply_to_message_id(),
                        "text": msg.text(),
                    }));
                }

                structured_result_with_text(
                    &json!({
                        "messages": messages,
                        "next_before_id": oldest_id,
                    }),
                    None,
                )
            }
            "search_messages" => {
                let input: SearchMessagesArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let client = self.ensure_client().await?;
                self.ensure_authorized(&client).await?;

                let peer_ref = Self::to_peer_ref(input.peer_ref)?;
                let desired = input.limit.unwrap_or(50).clamp(1, MAX_MESSAGES) as usize;

                let mut iter = client.search_messages(peer_ref).query(&input.query);
                if let Some(before_id) = input.before_id {
                    iter = iter.offset_id(before_id);
                }

                let mut messages = Vec::with_capacity(desired);
                let mut oldest_id: Option<i32> = None;

                while messages.len() < desired {
                    let Some(msg) = iter.next().await.map_err(|e| {
                        ConnectorError::Other(format!("Failed to search messages: {e}"))
                    })?
                    else {
                        break;
                    };

                    let sender_id = msg.sender().map(|p| p.id().bot_api_dialog_id());
                    let peer_id = msg.peer_id().bot_api_dialog_id();
                    let msg_id = msg.id();
                    oldest_id = Some(oldest_id.map_or(msg_id, |cur| cur.min(msg_id)));

                    messages.push(json!({
                        "id": msg_id,
                        "date": msg.date().to_rfc3339(),
                        "outgoing": msg.outgoing(),
                        "peer_id": peer_id,
                        "sender_id": sender_id,
                        "reply_to": msg.reply_to_message_id(),
                        "text": msg.text(),
                    }));
                }

                structured_result_with_text(
                    &json!({
                        "messages": messages,
                        "next_before_id": oldest_id,
                    }),
                    None,
                )
            }
            "send_message" => {
                let input: SendMessageArgs = serde_json::from_value(Value::Object(args_map))
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let client = self.ensure_client().await?;
                self.ensure_authorized(&client).await?;

                let peer_ref = Self::to_peer_ref(input.peer_ref)?;
                let msg = client
                    .send_message(peer_ref, input.message)
                    .await
                    .map_err(|e| ConnectorError::Other(format!("Failed to send message: {e}")))?;

                structured_result_with_text(
                    &json!({
                        "sent": true,
                        "id": msg.id(),
                        "date": msg.date().to_rfc3339(),
                    }),
                    None,
                )
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
        self.reset_runtime().await;
        if !self.auth.is_empty() {
            let store = FileAuthStore::new_default();
            let _ = store.save(self.name(), &details);
        }
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let client = self.ensure_client().await?;
        let ok = client
            .is_authorized()
            .await
            .map_err(|e| ConnectorError::Other(format!("Telegram auth check failed: {e}")))?;
        if ok {
            Ok(())
        } else {
            Err(ConnectorError::Authentication(
                "Telegram session not authorized".to_string(),
            ))
        }
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![
                Field {
                    name: "api_id".to_string(),
                    label: "Telegram API ID".to_string(),
                    field_type: FieldType::Number,
                    required: true,
                    description: Some(
                        "Telegram developer API ID from https://my.telegram.org/apps (TG_ID)."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "api_hash".to_string(),
                    label: "Telegram API Hash".to_string(),
                    field_type: FieldType::Secret,
                    required: true,
                    description: Some(
                        "Telegram developer API hash from https://my.telegram.org/apps (TG_HASH)."
                            .to_string(),
                    ),
                    options: None,
                },
                Field {
                    name: "session_file".to_string(),
                    label: "Session File (optional)".to_string(),
                    field_type: FieldType::Text,
                    required: false,
                    description: Some(
                        "Path to the local Telegram session file (default ~/.config/rzn-tools/telegram.session)."
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
    fn parses_bot_api_dialog_id_to_peer_id() {
        let user = peer_id_from_bot_api_dialog_id(123).unwrap();
        assert_eq!(user.kind(), PeerKind::User);

        let chat = peer_id_from_bot_api_dialog_id(-42).unwrap();
        assert_eq!(chat.kind(), PeerKind::Chat);

        let channel = peer_id_from_bot_api_dialog_id(-1000000000000 - 1).unwrap();
        assert_eq!(channel.kind(), PeerKind::Channel);
    }
}
