use crate::capabilities::{ConnectorConfigSchema, Field, FieldType};
use crate::error::ConnectorError;
use crate::ingest::{
    self, Author, ContentBlock, ContentItem, NormalizedItemV1, NormalizedPageV1, OutputFormat,
    Partial, Relationship, Source, Truncation,
};
use crate::utils::{structured_result, structured_result_with_text};
use crate::{auth::AuthDetails, Connector};
use async_trait::async_trait;
use chrono::{Datelike, NaiveDate, TimeZone, Utc};
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use serenity::http::{Http, MessagePagination};
use serenity::model::id::{ChannelId, GuildId, MessageId};
use std::borrow::Cow;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct ReadMessagesArgs {
    channel_id: u64,
    limit: Option<u64>,
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IngestDailyWindowsArgs {
    guild_id: u64,
    channel_id: u64,
    #[serde(default)]
    days: Option<u64>,
    #[serde(default)]
    start_days_ago: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IngestRollingLastNArgs {
    guild_id: u64,
    channel_id: u64,
    #[serde(default)]
    last_n: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IngestBackfillWindowsArgs {
    guild_id: u64,
    channel_id: u64,
    #[serde(default)]
    window_size: Option<u64>,
    #[serde(default)]
    page_limit: Option<u64>,
    #[serde(default)]
    limit: Option<u64>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GetIngestItemArgs {
    item_ref: String,
    #[serde(default)]
    max_messages: Option<u64>,
    #[serde(default)]
    max_pages: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SendMessageArgs {
    channel_id: u64,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ListChannelsArgs {
    guild_id: u64,
}

#[derive(Debug, Deserialize)]
struct GetServerInfoArgs {
    guild_id: u64,
}

#[derive(Debug, Deserialize)]
struct SearchMessagesArgs {
    channel_id: u64,
    query: String,
    limit: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct DiscordCursor {
    before: u64,
}

#[derive(Debug, Clone)]
enum DiscordIngestRef {
    Daily {
        guild_id: u64,
        channel_id: u64,
        day: NaiveDate,
    },
    Rolling {
        guild_id: u64,
        channel_id: u64,
        last_n: u64,
    },
    Range {
        guild_id: u64,
        channel_id: u64,
        start_id: u64,
        end_id: u64,
    },
}

pub struct DiscordConnector {
    http: Option<Arc<Http>>,
    token: Option<String>,
}

impl DiscordConnector {
    pub async fn new(auth: AuthDetails) -> Result<Self, ConnectorError> {
        let mut connector = Self {
            http: None,
            token: None,
        };
        if !auth.is_empty() {
            connector.set_auth_details(auth).await?;
        }
        Ok(connector)
    }

    fn get_http(&self) -> Result<&Arc<Http>, ConnectorError> {
        self.http.as_ref().ok_or(ConnectorError::Authentication(
            "Discord token not provided".to_string(),
        ))
    }
}

#[async_trait]
impl Connector for DiscordConnector {
    fn name(&self) -> &'static str {
        "discord"
    }

    fn description(&self) -> &'static str {
        "Interact with Discord servers, channels, and messages"
    }

    fn display_name(&self) -> &'static str {
        "Discord"
    }

    fn icon(&self) -> &'static str {
        "discord"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["communication", "community"]
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

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        let mut auth = AuthDetails::new();
        if let Some(token) = &self.token {
            auth.insert("token".to_string(), token.clone());
        }
        Ok(auth)
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        if let Some(token) = details.get("token").or(details.get("bot_token")) {
            self.token = Some(token.clone());
            self.http = Some(Arc::new(Http::new(token)));
            Ok(())
        } else {
            // Maybe it's in env?
            if let Ok(token) = std::env::var("DISCORD_TOKEN") {
                self.token = Some(token.clone());
                self.http = Some(Arc::new(Http::new(&token)));
                Ok(())
            } else {
                Err(ConnectorError::Authentication(
                    "Missing 'token' in auth details".to_string(),
                ))
            }
        }
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        let http = self.get_http()?;
        match http.get_current_user().await {
            Ok(_) => Ok(()),
            Err(e) => Err(ConnectorError::Authentication(format!(
                "Auth failed: {}",
                e
            ))),
        }
    }

    fn config_schema(&self) -> ConnectorConfigSchema {
        ConnectorConfigSchema {
            fields: vec![Field {
                name: "token".to_string(),
                label: "Bot Token".to_string(),
                field_type: FieldType::Secret,
                required: true,
                description: Some("Discord Bot Token".to_string()),
                options: None,
            }],
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
            instructions: Some("Access Discord. Requires a Bot Token and MESSAGE_CONTENT intent enabled in Discord Developer Portal for reading message content.".to_string()),
        })
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            Tool {
                name: Cow::Borrowed("list_servers"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List servers (guilds) the bot can access. Use when you need a guild_id.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {},
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_server_info"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get server details by guild_id. Use after list_servers. Example: guild_id=123.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "guild_id": { "type": "integer", "description": "ID of the server/guild" }
                    },
                    "required": ["guild_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("list_channels"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List channels in a server. Use when you need a channel_id. Example: guild_id=123.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "guild_id": { "type": "integer", "description": "ID of the server/guild" }
                    },
                    "required": ["guild_id"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("ingest_daily_windows"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Discovery tool for ingestion: one stable item per day for a (guild_id, channel_id). Defaults to yesterday only.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "guild_id": { "type": "integer", "description": "ID of the server/guild" },
                        "channel_id": { "type": "integer", "description": "ID of the channel" },
                        "days": { "type": "integer", "minimum": 1, "maximum": 30, "default": 1, "description": "How many day windows to return." },
                        "start_days_ago": { "type": "integer", "minimum": 1, "maximum": 3650, "default": 1, "description": "How many days ago to start (1 = yesterday)." },
                        "output_format": { "type": "string", "enum": ["raw", "normalized_v1", "display_v1"], "default": "raw" }
                    },
                    "required": ["guild_id", "channel_id"],
                    "examples": [
                        {
                            "description": "Discover yesterday's daily window (normalized)",
                            "input": { "guild_id": 123456789, "channel_id": 987654321, "output_format": "normalized_v1" }
                        },
                        {
                            "description": "Discover last 7 daily windows (raw metadata)",
                            "input": { "guild_id": 123456789, "channel_id": 987654321, "days": 7, "start_days_ago": 1, "output_format": "raw" }
                        }
                    ],
                    "_meta": {
                        "category": "list",
                        "tags": ["communication", "chat", "ingest"],
                        "auth_required": true,
                        "supports_output_format": true,
                        "supports_cursor": false
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("ingest_rolling_last_n"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Discovery tool for ingestion: one stable item per channel representing a rolling snapshot of the last N messages. item_ref is stable; content_hash changes when new messages arrive.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "guild_id": { "type": "integer", "description": "ID of the server/guild" },
                        "channel_id": { "type": "integer", "description": "ID of the channel" },
                        "last_n": { "type": "integer", "minimum": 1, "maximum": 5000, "default": 500, "description": "Number of most recent messages to represent." },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 5000, "description": "Alias for last_n." },
                        "output_format": { "type": "string", "enum": ["raw", "normalized_v1", "display_v1"], "default": "raw" }
                    },
                    "required": ["guild_id", "channel_id"],
                    "examples": [
                        {
                            "description": "Discover rolling item for last 500 messages (normalized)",
                            "input": { "guild_id": 123456789, "channel_id": 987654321, "last_n": 500, "output_format": "normalized_v1" }
                        }
                    ],
                    "_meta": {
                        "category": "list",
                        "tags": ["communication", "chat", "ingest"],
                        "auth_required": true,
                        "supports_output_format": true,
                        "supports_cursor": false
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("ingest_backfill_windows"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Discovery tool for ingestion: cursor-driven backfill using windowed items (newest-first). Returns message-id ranges and a next_cursor to continue.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "guild_id": { "type": "integer", "description": "ID of the server/guild" },
                        "channel_id": { "type": "integer", "description": "ID of the channel" },
                        "window_size": { "type": "integer", "minimum": 1, "maximum": 100, "default": 100, "description": "Messages per window (Discord API max 100)." },
                        "page_limit": { "type": "integer", "minimum": 1, "maximum": 25, "default": 3, "description": "How many windows to return per call." },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 25, "description": "Alias for page_limit (required by the ingestion cursor schema)." },
                        "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous response (fetch older windows)." },
                        "output_format": { "type": "string", "enum": ["raw", "normalized_v1", "display_v1"], "default": "raw" }
                    },
                    "required": ["guild_id", "channel_id"],
                    "examples": [
                        {
                            "description": "Start backfill (newest-first) with normalized output",
                            "input": { "guild_id": 123456789, "channel_id": 987654321, "page_limit": 3, "window_size": 100, "output_format": "normalized_v1" }
                        },
                        {
                            "description": "Continue backfill using cursor",
                            "input": { "guild_id": 123456789, "channel_id": 987654321, "cursor": "opaque-cursor", "page_limit": 3, "output_format": "normalized_v1" }
                        }
                    ],
                    "_meta": {
                        "category": "list",
                        "tags": ["communication", "chat", "ingest"],
                        "auth_required": true,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("read_messages"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Read recent messages in a channel. Use when you want context. Example: channel_id=456 limit=50.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "channel_id": { "type": "integer", "description": "ID of the channel" },
                        "limit": { "type": "integer", "description": "Number of messages (max 100)" },
                        "cursor": { "type": ["string", "null"], "description": "Opaque cursor from a previous response (fetch older messages)." },
                        "output_format": { "type": "string", "enum": ["raw", "normalized_v1", "display_v1"], "default": "raw", "description": "Default raw. Use normalized_v1 for ingestion pipelines. Use display_v1 for UI-friendly output." }
                    },
                    "required": ["channel_id"],
                    "examples": [
                        {
                            "description": "Read recent messages",
                            "input": { "channel_id": 123456789, "limit": 50 }
                        },
                        {
                            "description": "Page older messages",
                            "input": { "channel_id": 123456789, "cursor": "opaque-cursor", "limit": 50 }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["communication", "chat"],
                        "auth_required": true,
                        "supports_output_format": true,
                        "supports_cursor": true
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Fetch a normalized discord item by item_ref (daily/rolling/backfill windows). Used by ingestion pipelines after discovery.",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "item_ref": { "type": "string", "description": "Ingest item_ref emitted by discord ingest_* tools." },
                        "max_messages": { "type": "integer", "minimum": 1, "maximum": 5000, "default": 5000, "description": "Hard cap on returned messages." },
                        "max_pages": { "type": "integer", "minimum": 1, "maximum": 500, "default": 200, "description": "Hard cap on Discord API pages." },
                        "output_format": { "type": "string", "enum": ["raw", "normalized_v1", "display_v1"], "default": "raw" }
                    },
                    "required": ["item_ref"],
                    "examples": [
                        {
                            "description": "Fetch a daily window by item_ref (normalized)",
                            "input": { "item_ref": "guild:123456789:channel:987654321:day:2025-01-01", "output_format": "normalized_v1" }
                        },
                        {
                            "description": "Fetch rolling last-N window by item_ref (normalized)",
                            "input": { "item_ref": "guild:123456789:channel:987654321:rolling:last_n=500", "output_format": "normalized_v1" }
                        }
                    ],
                    "_meta": {
                        "category": "read",
                        "tags": ["communication", "chat", "ingest"],
                        "auth_required": true,
                        "supports_output_format": true,
                        "supports_cursor": false
                    }
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("send_message"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Send a message to a channel. Use when you need to post as the bot. Example: channel_id=456 content=\"hello\".",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "channel_id": { "type": "integer", "description": "ID of the channel" },
                        "content": { "type": "string", "description": "Message content" }
                    },
                    "required": ["channel_id", "content"]
                }).as_object().expect("Schema object").clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("search_messages"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search messages in a channel by keyword. Use when you need matches, not chronology. Example: channel_id=456 query=\"deploy\".",
                )),
                input_schema: Arc::new(json!({
                    "type": "object",
                    "properties": {
                        "channel_id": { "type": "integer", "description": "ID of the channel" },
                        "query": { "type": "string", "description": "Text to search for within message content" },
                        "limit": { "type": "integer", "description": "Number of matching messages (max 100)" }
                    },
                    "required": ["channel_id", "query"]
                }).as_object().expect("Schema object").clone()),
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
        let http = self.get_http()?;

        match request.name.as_ref() {
            "list_servers" => {
                let guilds = http
                    .get_guilds(None, None)
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                let data: Vec<Value> = guilds
                    .iter()
                    .map(|g| {
                        json!({
                            "id": g.id.get(),
                            "name": g.name,
                        })
                    })
                    .collect();

                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            "get_server_info" => {
                let args: GetServerInfoArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let guild = http
                    .get_guild(GuildId::new(args.guild_id))
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                let data = json!({
                    "id": guild.id.get(),
                    "name": guild.name,
                    "description": guild.description,
                    "member_count": guild.approximate_member_count,
                    "owner_id": guild.owner_id.get(),
                });
                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            "list_channels" => {
                let args: ListChannelsArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let channels = http
                    .get_channels(GuildId::new(args.guild_id))
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                let data: Vec<Value> = channels
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id.get(),
                            "name": c.name,
                            "type": format!("{:?}", c.kind),
                        })
                    })
                    .collect();
                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            "ingest_daily_windows" => {
                let args_value = serde_json::to_value(request.arguments.unwrap_or_default())
                    .map_err(ConnectorError::SerdeJson)?;
                let args_map = args_value.as_object().cloned().unwrap_or_default();
                let output_format = ingest::output_format_from_args(&args_map)?;
                let args: IngestDailyWindowsArgs = serde_json::from_value(args_value)
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let days = args.days.unwrap_or(1).clamp(1, 30);
                let start_days_ago = args.start_days_ago.unwrap_or(1).max(1);
                let today = Utc::now().date_naive();

                if output_format != OutputFormat::NormalizedV1 {
                    let data = json!({
                        "mode": "daily_windows",
                        "guild_id": args.guild_id,
                        "channel_id": args.channel_id,
                        "days": days,
                        "start_days_ago": start_days_ago,
                    });
                    return structured_result_with_text(&data, Some(serde_json::to_string(&data)?));
                }

                let channel_url = Some(format!(
                    "https://discord.com/channels/{}/{}",
                    args.guild_id, args.channel_id
                ));

                let mut items: Vec<ContentItem> = Vec::with_capacity(days as usize);
                for i in 0..days {
                    let day = today - chrono::Duration::days((start_days_ago + i) as i64);
                    let date_str = day.format("%Y-%m-%d").to_string();
                    let item_ref = format!(
                        "guild:{}:channel:{}:day:{}",
                        args.guild_id, args.channel_id, date_str
                    );
                    let created_at = Utc
                        .from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap())
                        .to_rfc3339();

                    items.push(ContentItem {
                        item_ref: item_ref.clone(),
                        kind: "daily_window".to_string(),
                        canonical_url: channel_url.clone(),
                        title: Some(format!(
                            "Discord channel {} daily window {}",
                            args.channel_id, date_str
                        )),
                        created_at: Some(created_at.clone()),
                        source_updated_at: Some(created_at),
                        authors: Vec::new(),
                        tags: Vec::new(),
                        metadata: Some(json!({
                            "mode": "daily",
                            "guild_id": args.guild_id,
                            "channel_id": args.channel_id,
                            "day": date_str,
                            "content_hash": item_ref,
                        })),
                        blocks: Vec::new(),
                        relationships: Vec::new(),
                        truncation: None,
                    });
                }

                let normalized = NormalizedPageV1::new(
                    items,
                    None,
                    false,
                    Partial::complete(Some(ingest::limits_max_items(days))),
                    Source::new("discord", "ingest_daily_windows"),
                );
                Ok(structured_result(&normalized)?)
            }
            "ingest_rolling_last_n" => {
                let args_value = serde_json::to_value(request.arguments.unwrap_or_default())
                    .map_err(ConnectorError::SerdeJson)?;
                let args_map = args_value.as_object().cloned().unwrap_or_default();
                let output_format = ingest::output_format_from_args(&args_map)?;
                let args: IngestRollingLastNArgs = serde_json::from_value(args_value)
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let last_n = args.last_n.or(args.limit).unwrap_or(500).clamp(1, 5_000);

                let mut last_message_id: Option<u64> = None;
                let mut last_message_at: Option<String> = None;

                let latest = http
                    .get_messages(ChannelId::new(args.channel_id), None, Some(1))
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;
                if let Some(m) = latest.first() {
                    last_message_id = Some(m.id.get());
                    last_message_at = m.timestamp.to_rfc3339();
                }

                let item_ref = format!(
                    "guild:{}:channel:{}:rolling:last_n={}",
                    args.guild_id, args.channel_id, last_n
                );
                let channel_url = Some(format!(
                    "https://discord.com/channels/{}/{}",
                    args.guild_id, args.channel_id
                ));

                let content_hash = match last_message_id {
                    Some(id) => format!("last:{}:n:{}", id, last_n),
                    None => format!("empty:n:{}", last_n),
                };

                if output_format != OutputFormat::NormalizedV1 {
                    let data = json!({
                        "mode": "rolling_last_n",
                        "guild_id": args.guild_id,
                        "channel_id": args.channel_id,
                        "last_n": last_n,
                        "item_ref": item_ref,
                        "content_hash": content_hash,
                        "last_message_id": last_message_id,
                        "last_message_at": last_message_at,
                    });
                    return structured_result_with_text(&data, Some(serde_json::to_string(&data)?));
                }

                let item = ContentItem {
                    item_ref: item_ref.clone(),
                    kind: "rolling_window".to_string(),
                    canonical_url: channel_url,
                    title: Some(format!(
                        "Discord channel {} rolling last {} messages",
                        args.channel_id, last_n
                    )),
                    created_at: last_message_at.clone(),
                    source_updated_at: last_message_at,
                    authors: Vec::new(),
                    tags: Vec::new(),
                    metadata: Some(json!({
                        "mode": "rolling",
                        "guild_id": args.guild_id,
                        "channel_id": args.channel_id,
                        "last_n": last_n,
                        "content_hash": content_hash,
                        "last_message_id": last_message_id,
                    })),
                    blocks: Vec::new(),
                    relationships: Vec::new(),
                    truncation: None,
                };

                let normalized = NormalizedPageV1::new(
                    vec![item],
                    None,
                    false,
                    Partial::complete(Some(ingest::limits_max_items(1))),
                    Source::new("discord", "ingest_rolling_last_n"),
                );
                Ok(structured_result(&normalized)?)
            }
            "ingest_backfill_windows" => {
                let args_value = serde_json::to_value(request.arguments.unwrap_or_default())
                    .map_err(ConnectorError::SerdeJson)?;
                let args_map = args_value.as_object().cloned().unwrap_or_default();
                let output_format = ingest::output_format_from_args(&args_map)?;
                let args: IngestBackfillWindowsArgs = serde_json::from_value(args_value)
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let window_size = args.window_size.unwrap_or(100).clamp(1, 100) as u8;
                let page_limit = args.page_limit.or(args.limit).unwrap_or(3).clamp(1, 25);

                let mut pagination: Option<MessagePagination> = None;
                if let Some(cursor) = args.cursor.as_deref() {
                    if let Some(decoded) = ingest::decode_cursor::<DiscordCursor>(cursor) {
                        pagination =
                            Some(MessagePagination::Before(MessageId::new(decoded.before)));
                    } else if let Ok(raw_id) = cursor.parse::<u64>() {
                        pagination = Some(MessagePagination::Before(MessageId::new(raw_id)));
                    } else {
                        return Err(ConnectorError::InvalidParams(
                            "Invalid cursor: expected opaque cursor or message id".to_string(),
                        ));
                    }
                }

                if output_format != OutputFormat::NormalizedV1 {
                    let data = json!({
                        "mode": "backfill_windows",
                        "guild_id": args.guild_id,
                        "channel_id": args.channel_id,
                        "window_size": window_size,
                        "page_limit": page_limit,
                        "cursor": args.cursor,
                    });
                    return structured_result_with_text(&data, Some(serde_json::to_string(&data)?));
                }

                let channel_url = Some(format!(
                    "https://discord.com/channels/{}/{}",
                    args.guild_id, args.channel_id
                ));

                let mut items: Vec<ContentItem> = Vec::new();
                let mut next_cursor: Option<String> = None;
                let mut has_more = false;

                let mut last_oldest: Option<u64> = None;
                let mut last_full: bool = false;

                for _ in 0..page_limit {
                    let messages = http
                        .get_messages(
                            ChannelId::new(args.channel_id),
                            pagination,
                            Some(window_size),
                        )
                        .await
                        .map_err(|e| ConnectorError::Other(e.to_string()))?;

                    if messages.is_empty() {
                        break;
                    }

                    let newest_id = messages.first().map(|m| m.id.get()).unwrap_or(0);
                    let oldest_id = messages.last().map(|m| m.id.get()).unwrap_or(0);
                    last_oldest = Some(oldest_id);
                    last_full = messages.len() as u8 >= window_size;

                    let item_ref = format!(
                        "guild:{}:channel:{}:range:{}-{}",
                        args.guild_id, args.channel_id, oldest_id, newest_id
                    );

                    let created_at = messages.last().and_then(|m| m.timestamp.to_rfc3339());
                    let updated_at = messages.first().and_then(|m| m.timestamp.to_rfc3339());

                    items.push(ContentItem {
                        item_ref: item_ref.clone(),
                        kind: "backfill_range".to_string(),
                        canonical_url: channel_url.clone(),
                        title: Some(format!(
                            "Discord channel {} messages {}-{}",
                            args.channel_id, oldest_id, newest_id
                        )),
                        created_at,
                        source_updated_at: updated_at,
                        authors: Vec::new(),
                        tags: Vec::new(),
                        metadata: Some(json!({
                            "mode": "backfill",
                            "guild_id": args.guild_id,
                            "channel_id": args.channel_id,
                            "range_start": oldest_id,
                            "range_end": newest_id,
                            "window_size": window_size,
                            "content_hash": item_ref,
                        })),
                        blocks: Vec::new(),
                        relationships: Vec::new(),
                        truncation: None,
                    });

                    pagination = Some(MessagePagination::Before(MessageId::new(oldest_id)));
                }

                if let Some(oldest) = last_oldest {
                    if last_full {
                        next_cursor = ingest::encode_cursor(&DiscordCursor { before: oldest }).ok();
                        has_more = next_cursor.is_some();
                    }
                }

                let partial =
                    Partial::complete(Some(ingest::limits_window_size(window_size as u64)));
                let normalized = NormalizedPageV1::new(
                    items,
                    next_cursor,
                    has_more,
                    partial,
                    Source::new("discord", "ingest_backfill_windows"),
                );
                Ok(structured_result(&normalized)?)
            }
            "read_messages" => {
                let args_value = serde_json::to_value(request.arguments.unwrap_or_default())
                    .map_err(ConnectorError::SerdeJson)?;
                let args_map = args_value.as_object().cloned().unwrap_or_default();
                let output_format = ingest::output_format_from_args(&args_map)?;
                let args: ReadMessagesArgs = serde_json::from_value(args_value)
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let limit = args.limit.unwrap_or(50).min(100);
                let pagination = if let Some(cursor) = args.cursor.as_deref() {
                    if let Some(decoded) = ingest::decode_cursor::<DiscordCursor>(cursor) {
                        Some(MessagePagination::Before(MessageId::new(decoded.before)))
                    } else if let Ok(raw_id) = cursor.parse::<u64>() {
                        Some(MessagePagination::Before(MessageId::new(raw_id)))
                    } else {
                        return Err(ConnectorError::InvalidParams(
                            "Invalid cursor: expected opaque cursor or message id".to_string(),
                        ));
                    }
                } else {
                    None
                };

                let messages = http
                    .get_messages(
                        ChannelId::new(args.channel_id),
                        pagination,
                        Some(limit as u8),
                    )
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                if output_format == OutputFormat::NormalizedV1 {
                    let newest_id = messages.first().map(|m| m.id.get());
                    let oldest_id = messages.last().map(|m| m.id.get());
                    let item_ref = match (oldest_id, newest_id) {
                        (Some(start), Some(end)) => {
                            format!(
                                "discord:channel_window:{}:{}-{}",
                                args.channel_id, start, end
                            )
                        }
                        _ => format!("discord:channel_window:{}", args.channel_id),
                    };

                    let mut blocks: Vec<ContentBlock> = Vec::new();
                    let mut relationships: Vec<Relationship> = Vec::new();

                    for msg in &messages {
                        let block_ref = format!("discord:message:{}", msg.id.get());
                        blocks.push(ContentBlock {
                            block_ref: block_ref.clone(),
                            block_kind: "message".to_string(),
                            text: msg.content.clone(),
                            author: Some(Author {
                                name: msg.author.name.clone(),
                                id: Some(format!("discord:user:{}", msg.author.id.get())),
                            }),
                            created_at: msg.timestamp.to_rfc3339(),
                            reply_to: None,
                            position: None,
                            score: None,
                            attachments: Vec::new(),
                            metadata: None,
                        });
                        relationships.push(Relationship {
                            rel: "has_block".to_string(),
                            from: item_ref.clone(),
                            to: block_ref,
                        });
                    }

                    let window_full = messages.len() as u64 >= limit;
                    let next_cursor = if window_full {
                        oldest_id.and_then(|id| {
                            ingest::encode_cursor(&DiscordCursor { before: id }).ok()
                        })
                    } else {
                        None
                    };
                    let has_more = next_cursor.is_some();

                    let truncation = if window_full {
                        Some(Truncation {
                            is_truncated: true,
                            reason: "window_limit".to_string(),
                            total_blocks_hint: None,
                            returned_blocks: blocks.len() as u64,
                            policy: Some("newest_first_window".to_string()),
                        })
                    } else {
                        None
                    };

                    let item = ContentItem {
                        item_ref: item_ref.clone(),
                        kind: "channel_window".to_string(),
                        canonical_url: None,
                        title: None,
                        created_at: None,
                        source_updated_at: None,
                        authors: Vec::new(),
                        tags: Vec::new(),
                        metadata: Some(json!({
                            "channel_id": args.channel_id,
                        })),
                        blocks,
                        relationships,
                        truncation,
                    };

                    let partial = if window_full {
                        Partial::truncated("window_limit", Some(ingest::limits_window_size(limit)))
                    } else {
                        Partial::complete(Some(ingest::limits_window_size(limit)))
                    };
                    let normalized = NormalizedPageV1::new(
                        vec![item],
                        next_cursor,
                        has_more,
                        partial,
                        Source::new("discord", "read_messages"),
                    );
                    return structured_result(&normalized);
                }

                let data: Vec<Value> = messages
                    .iter()
                    .map(|m| {
                        json!({
                            "id": m.id.get(),
                            "author": m.author.name,
                            "content": m.content,
                            "timestamp": m.timestamp.to_rfc3339(),
                        })
                    })
                    .collect();
                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            "get" => {
                let args_value = serde_json::to_value(request.arguments.unwrap_or_default())
                    .map_err(ConnectorError::SerdeJson)?;
                let args_map = args_value.as_object().cloned().unwrap_or_default();
                let output_format = ingest::output_format_from_args(&args_map)?;
                let args: GetIngestItemArgs = serde_json::from_value(args_value)
                    .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let ingest_ref = parse_discord_ingest_ref(&args.item_ref)?;
                let max_messages = args.max_messages.unwrap_or(5_000).clamp(1, 5_000) as usize;
                let max_pages = args.max_pages.unwrap_or(200).clamp(1, 500) as usize;

                if output_format != OutputFormat::NormalizedV1 {
                    let data = json!({
                        "item_ref": args.item_ref,
                        "parsed": format!("{:?}", ingest_ref),
                        "note": "Use output_format=normalized_v1 for ingestion."
                    });
                    return structured_result_with_text(&data, Some(serde_json::to_string(&data)?));
                }

                let item = match ingest_ref {
                    DiscordIngestRef::Rolling {
                        guild_id,
                        channel_id,
                        last_n,
                    } => {
                        fetch_rolling_item(
                            http,
                            guild_id,
                            channel_id,
                            last_n,
                            max_messages,
                            max_pages,
                        )
                        .await?
                    }
                    DiscordIngestRef::Range {
                        guild_id,
                        channel_id,
                        start_id,
                        end_id,
                    } => {
                        fetch_range_item(
                            http,
                            guild_id,
                            channel_id,
                            start_id,
                            end_id,
                            max_messages,
                            max_pages,
                        )
                        .await?
                    }
                    DiscordIngestRef::Daily {
                        guild_id,
                        channel_id,
                        day,
                    } => {
                        fetch_daily_item(http, guild_id, channel_id, day, max_messages, max_pages)
                            .await?
                    }
                };

                let normalized = NormalizedItemV1::complete(item, Source::new("discord", "get"));
                Ok(structured_result(&normalized)?)
            }
            "send_message" => {
                let args: SendMessageArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let map = json!({ "content": args.content });
                // Serenity 0.12: send_message(channel_id, files, map)
                let msg = http
                    .send_message(
                        ChannelId::new(args.channel_id),
                        Vec::<serenity::builder::CreateAttachment>::new(),
                        &map,
                    )
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                let data = json!({
                    "id": msg.id.get(),
                    "content": msg.content,
                    "status": "sent"
                });
                Ok(structured_result_with_text(
                    &data,
                    Some(serde_json::to_string(&data)?),
                )?)
            }
            "search_messages" => {
                let args: SearchMessagesArgs = serde_json::from_value(
                    serde_json::to_value(request.arguments.unwrap_or_default())
                        .map_err(ConnectorError::SerdeJson)?,
                )
                .map_err(|e| ConnectorError::InvalidParams(e.to_string()))?;

                let limit = args.limit.unwrap_or(50).min(100) as u8;
                let messages = http
                    .get_messages(ChannelId::new(args.channel_id), None, Some(limit))
                    .await
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                let query_lower = args.query.to_lowercase();
                let filtered_messages: Vec<Value> = messages
                    .iter()
                    .filter(|m| m.content.to_lowercase().contains(&query_lower))
                    .map(|m| {
                        json!({
                            "id": m.id.get(),
                            "author": m.author.name,
                            "content": m.content,
                            "timestamp": m.timestamp.to_rfc3339(),
                        })
                    })
                    .collect();
                Ok(structured_result_with_text(
                    &json!({"query": args.query, "results": filtered_messages}),
                    Some(serde_json::to_string(
                        &json!({"query": args.query, "results": filtered_messages}),
                    )?),
                )?)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
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

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::InvalidParams(format!(
            "Prompt '{}' not found",
            name
        )))
    }
}

fn parse_discord_ingest_ref(item_ref: &str) -> Result<DiscordIngestRef, ConnectorError> {
    let parts: Vec<&str> = item_ref.split(':').collect();
    if parts.len() < 6 {
        return Err(ConnectorError::InvalidParams(
            "Invalid item_ref: expected guild:<gid>:channel:<cid>:<mode>:...".to_string(),
        ));
    }

    if parts[0] != "guild" || parts[2] != "channel" {
        return Err(ConnectorError::InvalidParams(
            "Invalid item_ref: expected guild:<gid>:channel:<cid>:...".to_string(),
        ));
    }

    let guild_id = parts[1]
        .parse::<u64>()
        .map_err(|_| ConnectorError::InvalidParams("Invalid guild id".to_string()))?;
    let channel_id = parts[3]
        .parse::<u64>()
        .map_err(|_| ConnectorError::InvalidParams("Invalid channel id".to_string()))?;

    match parts[4] {
        "day" => {
            let day_str = parts[5];
            let day = NaiveDate::parse_from_str(day_str, "%Y-%m-%d").map_err(|_| {
                ConnectorError::InvalidParams("Invalid day: expected YYYY-MM-DD".to_string())
            })?;
            Ok(DiscordIngestRef::Daily {
                guild_id,
                channel_id,
                day,
            })
        }
        "rolling" => {
            let spec = parts[5];
            let last_n = spec
                .strip_prefix("last_n=")
                .ok_or_else(|| {
                    ConnectorError::InvalidParams(
                        "Invalid rolling item_ref: expected rolling:last_n=<N>".to_string(),
                    )
                })?
                .parse::<u64>()
                .map_err(|_| ConnectorError::InvalidParams("Invalid last_n".to_string()))?;
            Ok(DiscordIngestRef::Rolling {
                guild_id,
                channel_id,
                last_n,
            })
        }
        "range" => {
            let spec = parts[5];
            let (start_s, end_s) = spec.split_once('-').ok_or_else(|| {
                ConnectorError::InvalidParams(
                    "Invalid range item_ref: expected range:<start>-<end>".to_string(),
                )
            })?;
            let start_id = start_s
                .parse::<u64>()
                .map_err(|_| ConnectorError::InvalidParams("Invalid range start".to_string()))?;
            let end_id = end_s
                .parse::<u64>()
                .map_err(|_| ConnectorError::InvalidParams("Invalid range end".to_string()))?;
            Ok(DiscordIngestRef::Range {
                guild_id,
                channel_id,
                start_id,
                end_id,
            })
        }
        _ => Err(ConnectorError::InvalidParams(
            "Unsupported discord ingest item_ref mode".to_string(),
        )),
    }
}

async fn fetch_rolling_item(
    http: &Arc<Http>,
    guild_id: u64,
    channel_id: u64,
    last_n: u64,
    max_messages: usize,
    max_pages: usize,
) -> Result<ContentItem, ConnectorError> {
    let want = (last_n as usize).min(max_messages).min(5_000);
    let mut pagination: Option<MessagePagination> = None;
    let mut out: Vec<serenity::model::channel::Message> = Vec::new();

    let mut pages = 0usize;
    while out.len() < want && pages < max_pages {
        pages += 1;
        let remaining = want.saturating_sub(out.len());
        let page_size = remaining.min(100) as u8;
        let messages = http
            .get_messages(ChannelId::new(channel_id), pagination, Some(page_size))
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        if messages.is_empty() {
            break;
        }
        let oldest_id = messages.last().map(|m| m.id.get());
        out.extend(messages);
        if let Some(id) = oldest_id {
            pagination = Some(MessagePagination::Before(MessageId::new(id)));
        } else {
            break;
        }
    }

    build_discord_item(
        format!(
            "guild:{}:channel:{}:rolling:last_n={}",
            guild_id, channel_id, last_n
        ),
        "rolling_window",
        Some(format!(
            "https://discord.com/channels/{}/{}",
            guild_id, channel_id
        )),
        format!(
            "Discord channel {} rolling last {} messages",
            channel_id, last_n
        ),
        guild_id,
        channel_id,
        "rolling",
        json!({ "last_n": last_n }),
        out,
    )
}

async fn fetch_range_item(
    http: &Arc<Http>,
    guild_id: u64,
    channel_id: u64,
    start_id: u64,
    end_id: u64,
    max_messages: usize,
    max_pages: usize,
) -> Result<ContentItem, ConnectorError> {
    let mut pagination: Option<MessagePagination> = Some(MessagePagination::Before(
        MessageId::new(end_id.saturating_add(1)),
    ));
    let mut out: Vec<serenity::model::channel::Message> = Vec::new();
    let mut pages = 0usize;

    while pages < max_pages && out.len() < max_messages {
        pages += 1;
        let page_size = (max_messages - out.len()).min(100) as u8;
        let messages = http
            .get_messages(ChannelId::new(channel_id), pagination, Some(page_size))
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        if messages.is_empty() {
            break;
        }

        let oldest_id = messages.last().map(|m| m.id.get());
        for m in messages.into_iter() {
            let mid = m.id.get();
            if mid < start_id {
                break;
            }
            if mid <= end_id {
                out.push(m);
            }
        }

        if let Some(id) = oldest_id {
            if id < start_id {
                break;
            }
            pagination = Some(MessagePagination::Before(MessageId::new(id)));
        } else {
            break;
        }
    }

    build_discord_item(
        format!(
            "guild:{}:channel:{}:range:{}-{}",
            guild_id, channel_id, start_id, end_id
        ),
        "backfill_range",
        Some(format!(
            "https://discord.com/channels/{}/{}",
            guild_id, channel_id
        )),
        format!(
            "Discord channel {} messages {}-{}",
            channel_id, start_id, end_id
        ),
        guild_id,
        channel_id,
        "backfill",
        json!({ "range_start": start_id, "range_end": end_id }),
        out,
    )
}

async fn fetch_daily_item(
    http: &Arc<Http>,
    guild_id: u64,
    channel_id: u64,
    day: NaiveDate,
    max_messages: usize,
    max_pages: usize,
) -> Result<ContentItem, ConnectorError> {
    let day_start_dt = Utc.from_utc_datetime(&day.and_hms_opt(0, 0, 0).unwrap());
    let day_end_dt = day_start_dt + chrono::Duration::days(1);
    let day_start_ts = day_start_dt.timestamp();
    let day_end_ts = day_end_dt.timestamp();

    let mut pagination: Option<MessagePagination> = None;
    let mut out: Vec<serenity::model::channel::Message> = Vec::new();

    let mut pages = 0usize;
    while pages < max_pages && out.len() < max_messages {
        pages += 1;
        let page_size = (max_messages - out.len()).min(100) as u8;
        let messages = http
            .get_messages(ChannelId::new(channel_id), pagination, Some(page_size))
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        if messages.is_empty() {
            break;
        }

        let oldest_ts = messages.last().map(|m| m.timestamp.unix_timestamp());
        let oldest_id = messages.last().map(|m| m.id.get());

        for m in messages.into_iter() {
            let ts = m.timestamp.unix_timestamp();
            if ts < day_start_ts {
                break;
            }
            if ts >= day_start_ts && ts < day_end_ts {
                out.push(m);
            }
        }

        if let Some(ts) = oldest_ts {
            if ts < day_start_ts {
                break;
            }
        }

        if let Some(id) = oldest_id {
            pagination = Some(MessagePagination::Before(MessageId::new(id)));
        } else {
            break;
        }
    }

    let date_str = format!("{:04}-{:02}-{:02}", day.year(), day.month(), day.day());
    build_discord_item(
        format!("guild:{}:channel:{}:day:{}", guild_id, channel_id, date_str),
        "daily_window",
        Some(format!(
            "https://discord.com/channels/{}/{}",
            guild_id, channel_id
        )),
        format!("Discord channel {} daily window {}", channel_id, date_str),
        guild_id,
        channel_id,
        "daily",
        json!({ "day": date_str }),
        out,
    )
}

fn build_discord_item(
    item_ref: String,
    kind: &str,
    canonical_url: Option<String>,
    title: String,
    guild_id: u64,
    channel_id: u64,
    mode: &str,
    extra_meta: Value,
    mut messages: Vec<serenity::model::channel::Message>,
) -> Result<ContentItem, ConnectorError> {
    messages.reverse();

    let created_at = messages.first().and_then(|m| m.timestamp.to_rfc3339());
    let source_updated_at = messages.last().and_then(|m| m.timestamp.to_rfc3339());

    let mut blocks: Vec<ContentBlock> = Vec::with_capacity(messages.len());
    let mut relationships: Vec<Relationship> = Vec::with_capacity(messages.len());

    for msg in messages.iter() {
        let block_ref = format!("discord:message:{}", msg.id.get());
        blocks.push(ContentBlock {
            block_ref: block_ref.clone(),
            block_kind: "message".to_string(),
            text: msg.content.clone(),
            author: Some(Author {
                name: msg.author.name.clone(),
                id: Some(format!("discord:user:{}", msg.author.id.get())),
            }),
            created_at: msg.timestamp.to_rfc3339(),
            reply_to: msg
                .referenced_message
                .as_ref()
                .map(|rm| format!("discord:message:{}", rm.id.get())),
            position: None,
            score: None,
            attachments: Vec::new(),
            metadata: None,
        });
        relationships.push(Relationship {
            rel: "has_block".to_string(),
            from: item_ref.clone(),
            to: block_ref,
        });
    }

    let mut metadata = json!({
        "mode": mode,
        "guild_id": guild_id,
        "channel_id": channel_id,
    });
    if let Some(obj) = metadata.as_object_mut() {
        if let Some(extra_obj) = extra_meta.as_object() {
            for (k, v) in extra_obj.iter() {
                obj.insert(k.clone(), v.clone());
            }
        }
    }

    Ok(ContentItem {
        item_ref,
        kind: kind.to_string(),
        canonical_url,
        title: Some(title),
        created_at,
        source_updated_at,
        authors: Vec::new(),
        tags: Vec::new(),
        metadata: Some(metadata),
        blocks,
        relationships,
        truncation: None,
    })
}
