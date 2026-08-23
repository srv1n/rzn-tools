// Apple Messages Connector - Native Messages.app integration via AppleScript
// macOS only - interact with iMessage and SMS conversations
//
// Note: Messages.app has limited AppleScript support for privacy reasons.
// - Sending messages: Fully supported
// - Reading conversations: Limited (basic chat listing, requires Full Disk Access for history)
// - The Messages SQLite database at ~/Library/Messages/chat.db contains full history
//   but requires special permissions to access.

mod alias_store;

#[cfg(target_os = "macos")]
use crate::connectors::apple_common::{escape_applescript_string, run_applescript_output};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use alias_store::{AliasRecord, AliasSource, AliasStore, AliasStoreState};
use async_trait::async_trait;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;

/// Apple Messages connector - interact with Messages.app via AppleScript
#[derive(Default)]
pub struct AppleMessagesConnector;

impl AppleMessagesConnector {
    pub fn new() -> Self {
        Self {}
    }
}

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct SendMessageResult {
    success: bool,
    message: String,
    alias: String,
}

// ============================================================================
// AppleScript Generators
// ============================================================================

#[cfg(target_os = "macos")]
fn script_send_message(recipient: &str, message: &str) -> String {
    format!(
        r#"
tell application "Messages"
    set targetService to 1st service whose service type = iMessage
    set targetBuddy to buddy "{}" of targetService
    send "{}" to targetBuddy
    return "Message sent successfully"
end tell
"#,
        escape_applescript_string(recipient),
        escape_applescript_string(message)
    )
}

/// Chat listing with last message preview
#[derive(Debug, Serialize, Deserialize)]
struct ChatListing {
    /// Use this with get_recent_messages/send_message
    alias: String,
    /// Whether the alias was auto-generated or user-defined
    alias_source: String,
    /// iMessage or SMS
    service: String,
    /// Preview of last message
    last_message: String,
    /// When last message was sent
    last_message_date: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RawChatListing {
    chat_identifier: String,
    service: String,
    last_message: String,
    last_message_date: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MessageRecord {
    id: i64,
    text: String,
    date: String,
    direction: String,
    sender_alias: String,
    chat_alias: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AliasSummary {
    alias: String,
    source: String,
}

#[derive(Debug)]
struct MessageQueryFilters<'a> {
    chat_identifier: Option<&'a str>,
    since: Option<&'a str>,
    since_message_id: Option<i64>,
    limit: usize,
}

// List chats from SQLite database - more reliable than AppleScript
#[cfg(target_os = "macos")]
async fn list_chats_from_db(limit: usize) -> Result<Vec<RawChatListing>, ConnectorError> {
    use tokio::process::Command;

    let db_path = dirs::home_dir()
        .ok_or_else(|| ConnectorError::Other("Cannot find home directory".to_string()))?
        .join("Library/Messages/chat.db");

    if !db_path.exists() {
        return Err(ConnectorError::Other(
            "Messages database not found. Make sure Messages.app has been used.".to_string(),
        ));
    }

    // Get chats with their last message (truncated to 80 chars for preview)
    let query = format!(
        r#"
        SELECT
            c.chat_identifier,
            COALESCE(c.display_name, '') as display_name,
            COALESCE(c.service_name, '') as service_name,
            COALESCE(substr((SELECT m.text FROM message m
             JOIN chat_message_join cmj ON m.rowid = cmj.message_id
             WHERE cmj.chat_id = c.rowid
             ORDER BY m.date DESC LIMIT 1), 1, 80), '') as last_message,
            COALESCE((SELECT datetime(m.date/1000000000 + 978307200, 'unixepoch', 'localtime')
             FROM message m
             JOIN chat_message_join cmj ON m.rowid = cmj.message_id
             WHERE cmj.chat_id = c.rowid
             ORDER BY m.date DESC LIMIT 1), '') as last_message_date
        FROM chat c
        WHERE c.chat_identifier IS NOT NULL
        ORDER BY (SELECT m.date FROM message m
                  JOIN chat_message_join cmj ON m.rowid = cmj.message_id
                  WHERE cmj.chat_id = c.rowid
                  ORDER BY m.date DESC LIMIT 1) DESC
        LIMIT {}
        "#,
        limit
    );

    let output = Command::new("sqlite3")
        .arg("-json")
        .arg(&db_path)
        .arg(&query)
        .output()
        .await
        .map_err(|e| ConnectorError::Other(format!("Failed to query messages database: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("unable to open database") || stderr.contains("authorization denied") {
            return Err(ConnectorError::Other(
                "Cannot access Messages database. Grant Full Disk Access to your terminal/app in System Preferences > Security & Privacy > Privacy > Full Disk Access."
                    .to_string(),
            ));
        }
        return Err(ConnectorError::Other(format!(
            "Database query failed: {}",
            stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }

    let raw_chats: Vec<serde_json::Value> = serde_json::from_str(&stdout)
        .map_err(|e| ConnectorError::Other(format!("Failed to parse chats: {}", e)))?;

    // Convert to our struct for better display
    let chats: Vec<RawChatListing> = raw_chats
        .into_iter()
        .map(|c| RawChatListing {
            chat_identifier: c["chat_identifier"].as_str().unwrap_or("").to_string(),
            service: c["service_name"].as_str().unwrap_or("").to_string(),
            last_message: c["last_message"].as_str().unwrap_or("").to_string(),
            last_message_date: c["last_message_date"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    Ok(chats)
}

// For reading message history, we need to access the SQLite database
// This requires Full Disk Access permission
#[cfg(target_os = "macos")]
async fn read_recent_messages_from_db(
    filters: &MessageQueryFilters<'_>,
) -> Result<Vec<serde_json::Value>, ConnectorError> {
    use tokio::process::Command;

    let db_path = dirs::home_dir()
        .ok_or_else(|| ConnectorError::Other("Cannot find home directory".to_string()))?
        .join("Library/Messages/chat.db");

    if !db_path.exists() {
        return Err(ConnectorError::Other(
            "Messages database not found. Make sure Messages.app has been used.".to_string(),
        ));
    }

    let query = build_message_query(filters)?;

    let output = Command::new("sqlite3")
        .arg("-json")
        .arg(&db_path)
        .arg(&query)
        .output()
        .await
        .map_err(|e| ConnectorError::Other(format!("Failed to query messages database: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("unable to open database") || stderr.contains("authorization denied") {
            return Err(ConnectorError::Other(
                "Cannot access Messages database. Grant Full Disk Access to your terminal/app in System Preferences > Security & Privacy > Privacy > Full Disk Access."
                    .to_string(),
            ));
        }
        return Err(ConnectorError::Other(format!(
            "Database query failed: {}",
            stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Ok(Vec::new());
    }

    let messages: Vec<serde_json::Value> = serde_json::from_str(&stdout)
        .map_err(|e| ConnectorError::Other(format!("Failed to parse messages: {}", e)))?;

    Ok(messages)
}

#[cfg(target_os = "macos")]
fn build_message_query(filters: &MessageQueryFilters<'_>) -> Result<String, ConnectorError> {
    let mut clauses = vec!["m.text IS NOT NULL".to_string()];

    if let Some(chat_identifier) = filters.chat_identifier {
        clauses.push(format!(
            "c.chat_identifier = '{}'",
            sql_literal(chat_identifier)
        ));
    }

    if let Some(since) = filters.since {
        let sqlite_since = parse_since_argument(since)?;
        clauses.push(format!(
            "datetime(m.date/1000000000 + 978307200, 'unixepoch', 'localtime') >= '{}'",
            sql_literal(&sqlite_since)
        ));
    }

    if let Some(since_message_id) = filters.since_message_id {
        clauses.push(format!("m.rowid > {since_message_id}"));
    }

    Ok(format!(
        r#"
        SELECT
            m.rowid as id,
            COALESCE(m.text, '') as text,
            datetime(m.date/1000000000 + 978307200, 'unixepoch', 'localtime') as date,
            m.is_from_me,
            COALESCE(h.id, '') as sender,
            COALESCE(c.chat_identifier, '') as chat_identifier
        FROM message m
        LEFT JOIN handle h ON m.handle_id = h.rowid
        LEFT JOIN chat_message_join cmj ON m.rowid = cmj.message_id
        LEFT JOIN chat c ON cmj.chat_id = c.rowid
        WHERE {}
        ORDER BY m.date DESC
        LIMIT {}
        "#,
        clauses.join(" AND "),
        filters.limit
    ))
}

#[cfg(target_os = "macos")]
fn parse_since_argument(value: &str) -> Result<String, ConnectorError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ConnectorError::InvalidParams(
            "'since' cannot be empty".to_string(),
        ));
    }

    if let Ok(parsed) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(parsed
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string());
    }

    if let Ok(parsed) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
        return Ok(parsed.format("%Y-%m-%d %H:%M:%S").to_string());
    }

    if let Ok(parsed) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
        return Ok(parsed
            .and_hms_opt(0, 0, 0)
            .expect("midnight should be valid")
            .format("%Y-%m-%d %H:%M:%S")
            .to_string());
    }

    Err(ConnectorError::InvalidParams(
        "Invalid 'since'. Use RFC3339, 'YYYY-MM-DD HH:MM:SS', or 'YYYY-MM-DD'.".to_string(),
    ))
}

#[cfg(target_os = "macos")]
fn sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

// ============================================================================
// Parsing Functions
// ============================================================================

#[cfg(target_os = "macos")]
fn alias_source_label(source: &AliasSource) -> &'static str {
    match source {
        AliasSource::Auto => "auto",
        AliasSource::Manual => "manual",
    }
}

#[cfg(target_os = "macos")]
fn ensure_alias_record(
    aliases: &mut AliasStoreState,
    identifier: &str,
) -> Result<AliasRecord, ConnectorError> {
    aliases
        .ensure_alias_for_identifier(identifier)
        .map_err(ConnectorError::Other)
}

#[cfg(target_os = "macos")]
fn sanitize_chat_listings(
    chats: Vec<RawChatListing>,
    aliases: &mut AliasStoreState,
) -> Result<Vec<ChatListing>, ConnectorError> {
    chats
        .into_iter()
        .map(|chat| {
            let alias_record = ensure_alias_record(aliases, &chat.chat_identifier)?;
            Ok(ChatListing {
                alias: alias_record.alias,
                alias_source: alias_source_label(&alias_record.source).to_string(),
                service: chat.service,
                last_message: chat.last_message,
                last_message_date: chat.last_message_date,
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn sanitize_messages(
    raw_messages: Vec<serde_json::Value>,
    aliases: &mut AliasStoreState,
) -> Result<Vec<MessageRecord>, ConnectorError> {
    raw_messages
        .into_iter()
        .map(|message| {
            let chat_alias = message["chat_identifier"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(|identifier| ensure_alias_record(aliases, identifier))
                .transpose()?
                .map(|record| record.alias)
                .unwrap_or_else(|| "unknown-chat".to_string());

            let is_from_me = message["is_from_me"].as_i64().unwrap_or_default() == 1;
            let sender_alias = if is_from_me {
                "me".to_string()
            } else if let Some(sender) =
                message["sender"].as_str().filter(|value| !value.is_empty())
            {
                ensure_alias_record(aliases, sender)?.alias
            } else {
                chat_alias.clone()
            };

            Ok(MessageRecord {
                id: message["id"].as_i64().unwrap_or_default(),
                text: message["text"].as_str().unwrap_or("").to_string(),
                date: message["date"].as_str().unwrap_or("").to_string(),
                direction: if is_from_me {
                    "sent".to_string()
                } else {
                    "received".to_string()
                },
                sender_alias,
                chat_alias,
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn resolve_chat_identifier(
    args: &serde_json::Map<String, serde_json::Value>,
    aliases: &AliasStoreState,
) -> Result<Option<String>, ConnectorError> {
    if let Some(alias) = args.get("alias").and_then(|value| value.as_str()) {
        let record = aliases.resolve_alias(alias).ok_or_else(|| {
            ConnectorError::InvalidParams(format!("Unknown apple-messages alias '{alias}'"))
        })?;
        return Ok(Some(record.identifier));
    }

    Ok(None)
}

#[cfg(target_os = "macos")]
fn resolve_recipient_alias(
    args: &serde_json::Map<String, serde_json::Value>,
    aliases: &AliasStoreState,
) -> Result<String, ConnectorError> {
    if let Some(alias) = args.get("alias").and_then(|value| value.as_str()) {
        let record = aliases.resolve_alias(alias).ok_or_else(|| {
            ConnectorError::InvalidParams(format!("Unknown apple-messages alias '{alias}'"))
        })?;
        return Ok(record.identifier);
    }

    Err(ConnectorError::InvalidParams("Missing 'alias'".to_string()))
}

// ============================================================================
// Connector Implementation
// ============================================================================

#[async_trait]
impl crate::Connector for AppleMessagesConnector {
    fn name(&self) -> &'static str {
        "apple-messages"
    }

    fn description(&self) -> &'static str {
        "Apple Messages.app connector for macOS. Send iMessages and SMS. Read conversation history (requires Full Disk Access for message history). Works with phone numbers and email addresses."
    }

    fn display_name(&self) -> &'static str {
        "Apple Messages"
    }

    fn icon(&self) -> &'static str {
        "apple-messages"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["communication", "messages", "personal"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn get_auth_details(&self) -> Result<crate::auth::AuthDetails, ConnectorError> {
        Ok(crate::auth::AuthDetails::new())
    }

    async fn set_auth_details(
        &mut self,
        _details: crate::auth::AuthDetails,
    ) -> Result<(), ConnectorError> {
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        #[cfg(target_os = "macos")]
        {
            let _ = run_applescript_output(r#"tell application "Messages" to name"#).await?;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(ConnectorError::Other(
                "Apple Messages is only available on macOS".to_string(),
            ))
        }
    }

    fn config_schema(&self) -> crate::capabilities::ConnectorConfigSchema {
        crate::capabilities::ConnectorConfigSchema { fields: vec![] }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let tools = vec![
            // Chat Management
            Tool {
                name: Cow::Borrowed("list_chats"),
                title: Some("List Chats".to_string()),
                description: Some(Cow::Borrowed(
                    "List recent chat conversations sorted by last message time. Returns privacy-safe aliases instead of raw phone numbers/emails. Use alias with get_recent_messages or send_message. REQUIRES Full Disk Access.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "limit": {
                                "type": "integer",
                                "description": "Maximum chats to return. Default: 20.",
                                "default": 20
                            }
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
            // Reading Messages (requires Full Disk Access)
            Tool {
                name: Cow::Borrowed("get_recent_messages"),
                title: Some("Get Recent Messages".to_string()),
                description: Some(Cow::Borrowed(
                    "Read recent message history from Messages database. Accepts privacy-safe aliases and returns alias-only sender/chat metadata. REQUIRES Full Disk Access permission granted to your terminal/app.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "alias": {
                                "type": "string",
                                "description": "Privacy-safe alias from list_chats/list_aliases."
                            },
                            "since": {
                                "type": "string",
                                "description": "Only include messages on/after this timestamp. Supports RFC3339, YYYY-MM-DD HH:MM:SS, or YYYY-MM-DD."
                            },
                            "since_message_id": {
                                "type": "integer",
                                "description": "Only include messages with message rowid greater than this. Use the returned sync_cursor.latest_message_id for incremental sync."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum messages to return. Default: 50.",
                                "default": 50
                            }
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
            // Sending Messages
            Tool {
                name: Cow::Borrowed("send_message"),
                title: Some("Send Message".to_string()),
                description: Some(Cow::Borrowed(
                    "Send an iMessage or SMS to a configured alias.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "alias": {
                                "type": "string",
                                "description": "Privacy-safe alias from list_chats/list_aliases."
                            },
                            "message": {
                                "type": "string",
                                "description": "Message text to send. Required."
                            }
                        },
                        "required": ["alias", "message"]
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
                name: Cow::Borrowed("list_aliases"),
                title: Some("List Aliases".to_string()),
                description: Some(Cow::Borrowed(
                    "List configured Apple Messages aliases without exposing raw phone numbers/emails.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {}
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
                name: Cow::Borrowed("upsert_alias"),
                title: Some("Upsert Alias".to_string()),
                description: Some(Cow::Borrowed(
                    "Create or update a local alias for a raw Apple Messages phone number/email. Use this from a trusted human flow when you want a memorable alias like 'mom' or 'contractor-jane'.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "alias": {
                                "type": "string",
                                "description": "Alias to store, e.g. mom or vendor-ops."
                            },
                            "identifier": {
                                "type": "string",
                                "description": "Raw phone number or iMessage email for the alias."
                            }
                        },
                        "required": ["alias", "identifier"]
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
                name: Cow::Borrowed("remove_alias"),
                title: Some("Remove Alias".to_string()),
                description: Some(Cow::Borrowed(
                    "Remove a previously configured Apple Messages alias.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "alias": {
                                "type": "string",
                                "description": "Alias to remove."
                            }
                        },
                        "required": ["alias"]
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
        #[cfg(not(target_os = "macos"))]
        {
            let _ = request;
            return Err(ConnectorError::Other(
                "Apple Messages is only available on macOS".to_string(),
            ));
        }

        #[cfg(target_os = "macos")]
        {
            let name = request.name.as_ref();
            let args = request.arguments.unwrap_or_default();
            let alias_store = AliasStore::new_default();
            let mut alias_state = alias_store.load_state().map_err(ConnectorError::Other)?;

            match name {
                "list_chats" => {
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
                    let chats =
                        sanitize_chat_listings(list_chats_from_db(limit).await?, &mut alias_state)?;
                    alias_store
                        .save_state(&mut alias_state)
                        .map_err(ConnectorError::Other)?;

                    let output = if chats.is_empty() {
                        json!({"message": "No chats found."})
                    } else {
                        json!({
                            "chats": chats,
                            "hint": "Use get_recent_messages --alias <alias> to view a conversation without exposing phone numbers."
                        })
                    };

                    structured_result_with_text(&output, None)
                }

                "get_recent_messages" => {
                    let chat_identifier = resolve_chat_identifier(&args, &alias_state)?;
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
                    let since = args.get("since").and_then(|v| v.as_str());
                    let since_message_id = args.get("since_message_id").and_then(|value| {
                        value
                            .as_i64()
                            .or_else(|| value.as_u64().and_then(|v| i64::try_from(v).ok()))
                    });

                    let filters = MessageQueryFilters {
                        chat_identifier: chat_identifier.as_deref(),
                        since,
                        since_message_id,
                        limit,
                    };

                    let messages = sanitize_messages(
                        read_recent_messages_from_db(&filters).await?,
                        &mut alias_state,
                    )?;
                    alias_store
                        .save_state(&mut alias_state)
                        .map_err(ConnectorError::Other)?;

                    let latest_message_id = messages.iter().map(|message| message.id).max();
                    let latest_message_date = messages.first().map(|message| message.date.clone());

                    structured_result_with_text(
                        &json!({
                            "messages": messages,
                            "sync_cursor": {
                                "latest_message_id": latest_message_id,
                                "latest_message_date": latest_message_date
                            }
                        }),
                        None,
                    )
                }

                "send_message" => {
                    let recipient = resolve_recipient_alias(&args, &alias_state)?;
                    let message =
                        args.get("message")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'message'".to_string())
                            })?;

                    let output =
                        run_applescript_output(&script_send_message(&recipient, message)).await?;
                    let alias_record = ensure_alias_record(&mut alias_state, &recipient)?;
                    alias_store
                        .save_state(&mut alias_state)
                        .map_err(ConnectorError::Other)?;
                    let result = SendMessageResult {
                        success: true,
                        message: output,
                        alias: alias_record.alias,
                    };
                    structured_result_with_text(&result, None)
                }

                "list_aliases" => {
                    let aliases = alias_state
                        .list()
                        .into_iter()
                        .map(|record| AliasSummary {
                            alias: record.alias,
                            source: alias_source_label(&record.source).to_string(),
                        })
                        .collect::<Vec<_>>();
                    structured_result_with_text(&json!({ "aliases": aliases }), None)
                }

                "upsert_alias" => {
                    let alias = args
                        .get("alias")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("Missing 'alias'".to_string())
                        })?;
                    let identifier = args
                        .get("identifier")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("Missing 'identifier'".to_string())
                        })?;

                    let record = alias_state
                        .upsert_manual_alias(alias, identifier)
                        .map_err(ConnectorError::Other)?;
                    alias_store
                        .save_state(&mut alias_state)
                        .map_err(ConnectorError::Other)?;

                    structured_result_with_text(
                        &json!({
                            "alias": record.alias,
                            "source": alias_source_label(&record.source)
                        }),
                        None,
                    )
                }

                "remove_alias" => {
                    let alias = args
                        .get("alias")
                        .and_then(|value| value.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("Missing 'alias'".to_string())
                        })?;
                    let removed = alias_state.remove_alias(alias);
                    alias_store
                        .save_state(&mut alias_state)
                        .map_err(ConnectorError::Other)?;
                    structured_result_with_text(&json!({ "removed": removed }), None)
                }

                _ => Err(ConnectorError::ToolNotFound),
            }
        }
    }
}
