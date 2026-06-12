// Apple Mail Connector - Native Mail.app integration via AppleScript
// macOS only - uses system Mail.app with all configured accounts
//
// This connector provides access to Mail.app without requiring separate IMAP credentials.
// It works with all accounts configured in Mail.app (iCloud, Gmail, Exchange, etc.)

#[cfg(target_os = "macos")]
use crate::connectors::apple_common::{
    apple_connector_capabilities, escape_applescript_string, run_applescript_output,
};
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use async_trait::async_trait;
use rmcp::model::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;

/// Apple Mail connector - interact with Mail.app via AppleScript
#[derive(Default)]
pub struct AppleMailConnector;

impl AppleMailConnector {
    pub fn new() -> Self {
        Self {}
    }
}

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct MailAccount {
    /// Account name as shown in Mail.app
    name: String,
    /// Account ID for reference
    id: String,
    /// Email addresses associated with this account
    email_addresses: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Mailbox {
    /// Mailbox name (e.g., "INBOX", "Sent", "Drafts")
    name: String,
    /// Full path including account
    full_name: String,
    /// Number of unread messages
    unread_count: i32,
    /// Total message count
    message_count: i32,
    /// Parent account name
    account: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MailMessage {
    /// Unique message ID
    id: String,
    /// Email subject
    subject: String,
    /// Sender email/name
    sender: String,
    /// Date received (as string)
    date_received: String,
    /// Whether the message has been read
    is_read: bool,
    /// Whether the message is flagged
    is_flagged: bool,
    /// Recipients (To field)
    recipients: Vec<String>,
    /// CC recipients
    cc_recipients: Vec<String>,
    /// Mailbox containing this message
    mailbox: String,
    /// Account name
    account: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MailMessageContent {
    /// Basic message info
    #[serde(flatten)]
    message: MailMessage,
    /// Plain text content of the message
    content: String,
    /// Whether content was truncated
    truncated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct DraftResult {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SendResult {
    success: bool,
    message: String,
}

// ============================================================================
// AppleScript Generators
// ============================================================================

#[cfg(target_os = "macos")]
fn script_list_accounts() -> String {
    r#"
tell application "Mail"
    set output to ""
    repeat with acc in accounts
        set accName to name of acc
        set accId to id of acc
        set emails to email addresses of acc
        set emailList to ""
        repeat with em in emails
            if emailList is not "" then set emailList to emailList & ";"
            set emailList to emailList & em
        end repeat
        if output is not "" then set output to output & "|||"
        set output to output & accName & ":::" & accId & ":::" & emailList
    end repeat
    return output
end tell
"#
    .to_string()
}

#[cfg(target_os = "macos")]
fn script_list_mailboxes(account: Option<&str>) -> String {
    match account {
        Some(acc) => format!(
            r#"
tell application "Mail"
    set output to ""
    set acc to account "{}"
    repeat with mb in mailboxes of acc
        set mbName to name of mb
        set unreadCnt to unread count of mb
        set msgCnt to count of messages of mb
        if output is not "" then set output to output & "|||"
        set output to output & mbName & ":::" & unreadCnt & ":::" & msgCnt & ":::" & (name of acc)
    end repeat
    return output
end tell
"#,
            escape_applescript_string(acc)
        ),
        None => r#"
tell application "Mail"
    set output to ""
    repeat with acc in accounts
        repeat with mb in mailboxes of acc
            set mbName to name of mb
            set unreadCnt to unread count of mb
            set msgCnt to count of messages of mb
            if output is not "" then set output to output & "|||"
            set output to output & mbName & ":::" & unreadCnt & ":::" & msgCnt & ":::" & (name of acc)
        end repeat
    end repeat
    return output
end tell
"#
        .to_string(),
    }
}

#[cfg(target_os = "macos")]
fn script_list_messages(mailbox: &str, account: Option<&str>, limit: usize) -> String {
    let account_clause = match account {
        Some(acc) => format!(r#"of account "{}""#, escape_applescript_string(acc)),
        None => String::new(),
    };

    format!(
        r#"
tell application "Mail"
    set mb to mailbox "{}" {}
    set msgs to messages of mb
    set msgCount to count of msgs
    set maxCount to {limit}
    if msgCount < maxCount then set maxCount to msgCount

    set output to ""
    repeat with i from 1 to maxCount
        set msg to item i of msgs
        set msgId to id of msg
        set msgSubject to subject of msg
        set msgSender to sender of msg
        set msgDate to date received of msg as string
        set msgRead to read status of msg
        set msgFlagged to flagged status of msg

        -- Get recipients
        set recipList to ""
        repeat with r in to recipients of msg
            if recipList is not "" then set recipList to recipList & ";"
            set recipList to recipList & (address of r)
        end repeat

        if output is not "" then set output to output & "|||"
        set output to output & msgId & ":::" & msgSubject & ":::" & msgSender & ":::" & msgDate & ":::" & msgRead & ":::" & msgFlagged & ":::" & recipList
    end repeat
    return output
end tell
"#,
        escape_applescript_string(mailbox),
        account_clause,
        limit = limit
    )
}

#[cfg(target_os = "macos")]
fn script_get_message(message_id: &str) -> String {
    format!(
        r#"
tell application "Mail"
    set msg to message id {}
    set msgId to id of msg
    set msgSubject to subject of msg
    set msgSender to sender of msg
    set msgDate to date received of msg as string
    set msgRead to read status of msg
    set msgFlagged to flagged status of msg
    set msgContent to content of msg

    -- Get recipients
    set recipList to ""
    repeat with r in to recipients of msg
        if recipList is not "" then set recipList to recipList & ";"
        set recipList to recipList & (address of r)
    end repeat

    -- Get CC recipients
    set ccList to ""
    repeat with r in cc recipients of msg
        if ccList is not "" then set ccList to ccList & ";"
        set ccList to ccList & (address of r)
    end repeat

    -- Get mailbox info
    set mbName to name of mailbox of msg
    set accName to name of account of mailbox of msg

    return msgId & ":::" & msgSubject & ":::" & msgSender & ":::" & msgDate & ":::" & msgRead & ":::" & msgFlagged & ":::" & recipList & ":::" & ccList & ":::" & mbName & ":::" & accName & "|||CONTENT|||" & msgContent
end tell
"#,
        message_id
    )
}

#[cfg(target_os = "macos")]
fn script_search_messages(
    query: &str,
    mailbox: Option<&str>,
    account: Option<&str>,
    limit: usize,
) -> String {
    let scope = match (mailbox, account) {
        (Some(mb), Some(acc)) => format!(
            r#"in mailbox "{}" of account "{}""#,
            escape_applescript_string(mb),
            escape_applescript_string(acc)
        ),
        (Some(mb), None) => format!(r#"in mailbox "{}""#, escape_applescript_string(mb)),
        (None, Some(acc)) => format!(r#"in account "{}""#, escape_applescript_string(acc)),
        (None, None) => String::new(),
    };

    format!(
        r#"
tell application "Mail"
    set foundMsgs to (messages {} whose subject contains "{}" or sender contains "{}" or content contains "{}")
    set msgCount to count of foundMsgs
    set maxCount to {limit}
    if msgCount < maxCount then set maxCount to msgCount

    set output to ""
    repeat with i from 1 to maxCount
        set msg to item i of foundMsgs
        set msgId to id of msg
        set msgSubject to subject of msg
        set msgSender to sender of msg
        set msgDate to date received of msg as string
        set msgRead to read status of msg

        if output is not "" then set output to output & "|||"
        set output to output & msgId & ":::" & msgSubject & ":::" & msgSender & ":::" & msgDate & ":::" & msgRead
    end repeat
    return output
end tell
"#,
        scope,
        escape_applescript_string(query),
        escape_applescript_string(query),
        escape_applescript_string(query),
        limit = limit
    )
}

#[cfg(target_os = "macos")]
fn script_create_draft(
    to: &str,
    subject: &str,
    body: &str,
    cc: Option<&str>,
    bcc: Option<&str>,
) -> String {
    let cc_clause = match cc {
        Some(c) if !c.is_empty() => {
            let addresses: Vec<String> = c
                .split(',')
                .map(|a| {
                    format!(
                        r#"make new cc recipient at end of cc recipients with properties {{address:"{}"}}"#,
                        escape_applescript_string(a.trim())
                    )
                })
                .collect();
            addresses.join("\n            ")
        }
        _ => String::new(),
    };

    let bcc_clause = match bcc {
        Some(b) if !b.is_empty() => {
            let addresses: Vec<String> = b
                .split(',')
                .map(|a| {
                    format!(
                        r#"make new bcc recipient at end of bcc recipients with properties {{address:"{}"}}"#,
                        escape_applescript_string(a.trim())
                    )
                })
                .collect();
            addresses.join("\n            ")
        }
        _ => String::new(),
    };

    let to_recipients: Vec<String> = to
        .split(',')
        .map(|a| {
            format!(
                r#"make new to recipient at end of to recipients with properties {{address:"{}"}}"#,
                escape_applescript_string(a.trim())
            )
        })
        .collect();
    let to_clause = to_recipients.join("\n            ");

    format!(
        r#"
tell application "Mail"
    set newMessage to make new outgoing message with properties {{subject:"{}", content:"{}", visible:true}}
    tell newMessage
        {}
        {}
        {}
    end tell
    return "Draft created successfully"
end tell
"#,
        escape_applescript_string(subject),
        escape_applescript_string(body),
        to_clause,
        cc_clause,
        bcc_clause
    )
}

#[cfg(target_os = "macos")]
fn script_send_message(to: &str, subject: &str, body: &str, cc: Option<&str>) -> String {
    let cc_clause = match cc {
        Some(c) if !c.is_empty() => {
            let addresses: Vec<String> = c
                .split(',')
                .map(|a| {
                    format!(
                        r#"make new cc recipient at end of cc recipients with properties {{address:"{}"}}"#,
                        escape_applescript_string(a.trim())
                    )
                })
                .collect();
            addresses.join("\n            ")
        }
        _ => String::new(),
    };

    let to_recipients: Vec<String> = to
        .split(',')
        .map(|a| {
            format!(
                r#"make new to recipient at end of to recipients with properties {{address:"{}"}}"#,
                escape_applescript_string(a.trim())
            )
        })
        .collect();
    let to_clause = to_recipients.join("\n            ");

    format!(
        r#"
tell application "Mail"
    set newMessage to make new outgoing message with properties {{subject:"{}", content:"{}", visible:false}}
    tell newMessage
        {}
        {}
    end tell
    send newMessage
    return "Message sent successfully"
end tell
"#,
        escape_applescript_string(subject),
        escape_applescript_string(body),
        to_clause,
        cc_clause
    )
}

#[cfg(target_os = "macos")]
fn script_mark_read(message_id: &str, read: bool) -> String {
    format!(
        r#"
tell application "Mail"
    set msg to message id {}
    set read status of msg to {}
    return "Message marked as {}"
end tell
"#,
        message_id,
        if read { "true" } else { "false" },
        if read { "read" } else { "unread" }
    )
}

#[cfg(target_os = "macos")]
fn script_mark_flagged(message_id: &str, flagged: bool) -> String {
    format!(
        r#"
tell application "Mail"
    set msg to message id {}
    set flagged status of msg to {}
    return "Message {} flagged"
end tell
"#,
        message_id,
        if flagged { "true" } else { "false" },
        if flagged { "" } else { "un" }
    )
}

#[cfg(target_os = "macos")]
fn script_delete_message(message_id: &str) -> String {
    format!(
        r#"
tell application "Mail"
    set msg to message id {}
    delete msg
    return "Message deleted"
end tell
"#,
        message_id
    )
}

#[cfg(target_os = "macos")]
fn script_move_message(
    message_id: &str,
    target_mailbox: &str,
    target_account: Option<&str>,
) -> String {
    let target = match target_account {
        Some(acc) => format!(
            r#"mailbox "{}" of account "{}""#,
            escape_applescript_string(target_mailbox),
            escape_applescript_string(acc)
        ),
        None => format!(r#"mailbox "{}""#, escape_applescript_string(target_mailbox)),
    };

    format!(
        r#"
tell application "Mail"
    set msg to message id {}
    move msg to {}
    return "Message moved to {}"
end tell
"#,
        message_id, target, target_mailbox
    )
}

#[cfg(target_os = "macos")]
fn script_reply_to_message(message_id: &str, body: &str, reply_all: bool) -> String {
    let reply_type = if reply_all {
        "reply with opening window with properties {reply to all:true}"
    } else {
        "reply with opening window"
    };

    format!(
        r#"
tell application "Mail"
    set msg to message id {}
    set replyMsg to {}
    tell replyMsg
        set content to "{}" & return & return & content
    end tell
    return "Reply draft created"
end tell
"#,
        message_id,
        reply_type,
        escape_applescript_string(body)
    )
}

// ============================================================================
// Parsing Functions
// ============================================================================

#[cfg(target_os = "macos")]
fn parse_accounts(output: &str) -> Vec<MailAccount> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 3 {
                Some(MailAccount {
                    name: parts[0].to_string(),
                    id: parts[1].to_string(),
                    email_addresses: parts[2]
                        .split(';')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_mailboxes(output: &str) -> Vec<Mailbox> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 4 {
                Some(Mailbox {
                    name: parts[0].to_string(),
                    full_name: format!("{}/{}", parts[3], parts[0]),
                    unread_count: parts[1].parse().unwrap_or(0),
                    message_count: parts[2].parse().unwrap_or(0),
                    account: parts[3].to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_messages(output: &str, mailbox: &str, account: &str) -> Vec<MailMessage> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 7 {
                Some(MailMessage {
                    id: parts[0].to_string(),
                    subject: parts[1].to_string(),
                    sender: parts[2].to_string(),
                    date_received: parts[3].to_string(),
                    is_read: parts[4] == "true",
                    is_flagged: parts[5] == "true",
                    recipients: parts[6]
                        .split(';')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect(),
                    cc_recipients: Vec::new(),
                    mailbox: mailbox.to_string(),
                    account: account.to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_message_content(output: &str, max_content_len: usize) -> Option<MailMessageContent> {
    let parts: Vec<&str> = output.split("|||CONTENT|||").collect();
    if parts.len() != 2 {
        return None;
    }

    let meta_parts: Vec<&str> = parts[0].split(":::").collect();
    if meta_parts.len() < 10 {
        return None;
    }

    let mut content = parts[1].to_string();
    let truncated = if content.len() > max_content_len {
        content.truncate(max_content_len);
        true
    } else {
        false
    };

    Some(MailMessageContent {
        message: MailMessage {
            id: meta_parts[0].to_string(),
            subject: meta_parts[1].to_string(),
            sender: meta_parts[2].to_string(),
            date_received: meta_parts[3].to_string(),
            is_read: meta_parts[4] == "true",
            is_flagged: meta_parts[5] == "true",
            recipients: meta_parts[6]
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
            cc_recipients: meta_parts[7]
                .split(';')
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect(),
            mailbox: meta_parts[8].to_string(),
            account: meta_parts[9].to_string(),
        },
        content,
        truncated,
    })
}

#[cfg(target_os = "macos")]
fn parse_search_results(output: &str) -> Vec<MailMessage> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 5 {
                Some(MailMessage {
                    id: parts[0].to_string(),
                    subject: parts[1].to_string(),
                    sender: parts[2].to_string(),
                    date_received: parts[3].to_string(),
                    is_read: parts[4] == "true",
                    is_flagged: false,
                    recipients: Vec::new(),
                    cc_recipients: Vec::new(),
                    mailbox: String::new(),
                    account: String::new(),
                })
            } else {
                None
            }
        })
        .collect()
}

// ============================================================================
// Connector Implementation
// ============================================================================

#[async_trait]
impl crate::Connector for AppleMailConnector {
    fn name(&self) -> &'static str {
        "apple-mail"
    }

    fn description(&self) -> &'static str {
        "Apple Mail.app connector for macOS. Access all email accounts configured in Mail.app without separate credentials. Read, search, compose, and manage emails natively."
    }

    fn display_name(&self) -> &'static str {
        "Apple Mail"
    }

    fn icon(&self) -> &'static str {
        "apple-mail"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["email", "productivity", "personal"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    async fn capabilities(&self) -> ServerCapabilities {
        #[cfg(target_os = "macos")]
        {
            apple_connector_capabilities()
        }
        #[cfg(not(target_os = "macos"))]
        {
            ServerCapabilities::default()
        }
    }

    async fn get_auth_details(&self) -> Result<crate::auth::AuthDetails, ConnectorError> {
        Ok(crate::auth::AuthDetails::new())
    }

    async fn set_auth_details(
        &mut self,
        _details: crate::auth::AuthDetails,
    ) -> Result<(), ConnectorError> {
        Ok(()) // No auth needed - uses Mail.app's configured accounts
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        #[cfg(target_os = "macos")]
        {
            // Just verify Mail.app is accessible
            let _ = run_applescript_output(r#"tell application "Mail" to name"#).await?;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(ConnectorError::Other(
                "Apple Mail is only available on macOS".to_string(),
            ))
        }
    }

    fn config_schema(&self) -> crate::capabilities::ConnectorConfigSchema {
        crate::capabilities::ConnectorConfigSchema { fields: vec![] }
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
                title: Some("Apple Mail".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Native Mail.app integration. Works with all accounts configured in macOS Mail. First use may trigger a permission prompt."
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
        // Keep the surface small to reduce ambiguity and context bloat for agents.
        // Back-compat: additional legacy tools are still accepted in call_tool().
        let tools = vec![
            Tool {
                name: Cow::Borrowed("list_mailboxes"),
                title: Some("List Mailboxes".to_string()),
                description: Some(Cow::Borrowed(
                    "List mailboxes (folders) in Mail.app (requires explicit user permission). \
Example: account=\"iCloud\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "account": {
                                "type": "string",
                                "description": "Optional account name filter (e.g., \"iCloud\")."
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
            Tool {
                name: Cow::Borrowed("list_messages"),
                title: Some("List Messages".to_string()),
                description: Some(Cow::Borrowed(
                    "List message summaries in a mailbox (requires explicit user permission). \
Use get_message for full bodies. Example: mailbox=\"INBOX\" limit=20.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mailbox": { "type": "string", "description": "Mailbox name (e.g., INBOX)." },
                            "account": { "type": "string", "description": "Optional account name (required if mailbox is ambiguous)." },
                            "limit": { "type": "integer", "default": 20, "description": "Max messages (default 20, max 100)." }
                        },
                        "required": ["mailbox"]
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
                name: Cow::Borrowed("get_message"),
                title: Some("Get Message".to_string()),
                description: Some(Cow::Borrowed(
                    "Get one email by message_id (requires explicit user permission). Tip: set \
max_content_length to keep output small. Example: message_id=\"123\" max_content_length=8000.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "message_id": { "type": "string", "description": "Message ID from list_messages/search." },
                            "max_content_length": { "type": "integer", "default": 10000, "description": "Max characters of body to return." }
                        },
                        "required": ["message_id"]
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
                name: Cow::Borrowed("search"),
                title: Some("Search Emails".to_string()),
                description: Some(Cow::Borrowed(
                    "Search emails by keyword (requires explicit user permission). Use to find \
message IDs, then call get_message. Example: query=\"invoice\" mailbox=\"INBOX\" limit=10.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Keyword query." },
                            "mailbox": { "type": "string", "description": "Optional mailbox scope." },
                            "account": { "type": "string", "description": "Optional account scope." },
                            "limit": { "type": "integer", "default": 20, "description": "Max results." }
                        },
                        "required": ["query"]
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
                name: Cow::Borrowed("create_draft"),
                title: Some("Create Draft".to_string()),
                description: Some(Cow::Borrowed(
                    "Create a draft email in Mail.app for user review (requires explicit user \
permission). Prefer this over send_message.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "to": { "type": "string", "description": "Recipients (comma-separated)." },
                            "subject": { "type": "string" },
                            "body": { "type": "string" },
                            "cc": { "type": "string" },
                            "bcc": { "type": "string" }
                        },
                        "required": ["to", "subject", "body"]
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
                title: Some("Send Email".to_string()),
                description: Some(Cow::Borrowed(
                    "Send an email immediately (requires explicit user permission + explicit \
confirmation). If the user hasn't confirmed, use create_draft instead.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "to": { "type": "string", "description": "Recipients (comma-separated)." },
                            "subject": { "type": "string" },
                            "body": { "type": "string" },
                            "cc": { "type": "string" }
                        },
                        "required": ["to", "subject", "body"]
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
                "Apple Mail is only available on macOS".to_string(),
            ));
        }

        #[cfg(target_os = "macos")]
        {
            let name = request.name.as_ref();
            let args = request.arguments.unwrap_or_default();

            match name {
                "list_accounts" => {
                    let output = run_applescript_output(&script_list_accounts()).await?;
                    let accounts = parse_accounts(&output);
                    structured_result_with_text(&accounts, None)
                }

                "list_mailboxes" => {
                    let account = args.get("account").and_then(|v| v.as_str());
                    let output = run_applescript_output(&script_list_mailboxes(account)).await?;
                    let mailboxes = parse_mailboxes(&output);
                    structured_result_with_text(&mailboxes, None)
                }

                "list_messages" => {
                    let mailbox =
                        args.get("mailbox")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'mailbox'".to_string())
                            })?;
                    let account = args.get("account").and_then(|v| v.as_str());
                    let limit = args
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(20)
                        .min(100) as usize;

                    let output =
                        run_applescript_output(&script_list_messages(mailbox, account, limit))
                            .await?;
                    let messages = parse_messages(&output, mailbox, account.unwrap_or(""));
                    structured_result_with_text(&messages, None)
                }

                "get_message" => {
                    let message_id =
                        args.get("message_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'message_id'".to_string())
                            })?;
                    let max_len = args
                        .get("max_content_length")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10000) as usize;

                    let output = run_applescript_output(&script_get_message(message_id)).await?;
                    let message = parse_message_content(&output, max_len).ok_or_else(|| {
                        ConnectorError::Other("Failed to parse message".to_string())
                    })?;
                    structured_result_with_text(&message, None)
                }

                "search" => {
                    let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'query'".to_string())
                    })?;
                    let mailbox = args.get("mailbox").and_then(|v| v.as_str());
                    let account = args.get("account").and_then(|v| v.as_str());
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

                    let output = run_applescript_output(&script_search_messages(
                        query, mailbox, account, limit,
                    ))
                    .await?;
                    let results = parse_search_results(&output);
                    structured_result_with_text(&results, None)
                }

                "create_draft" => {
                    let to = args
                        .get("to")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ConnectorError::InvalidParams("Missing 'to'".to_string()))?;
                    let subject =
                        args.get("subject")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'subject'".to_string())
                            })?;
                    let body = args.get("body").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'body'".to_string())
                    })?;
                    let cc = args.get("cc").and_then(|v| v.as_str());
                    let bcc = args.get("bcc").and_then(|v| v.as_str());

                    let output =
                        run_applescript_output(&script_create_draft(to, subject, body, cc, bcc))
                            .await?;
                    let result = DraftResult {
                        success: true,
                        message: output,
                    };
                    structured_result_with_text(&result, None)
                }

                "send_message" => {
                    let to = args
                        .get("to")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| ConnectorError::InvalidParams("Missing 'to'".to_string()))?;
                    let subject =
                        args.get("subject")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'subject'".to_string())
                            })?;
                    let body = args.get("body").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'body'".to_string())
                    })?;
                    let cc = args.get("cc").and_then(|v| v.as_str());

                    let output =
                        run_applescript_output(&script_send_message(to, subject, body, cc)).await?;
                    let result = SendResult {
                        success: true,
                        message: output,
                    };
                    structured_result_with_text(&result, None)
                }

                "reply" => {
                    let message_id =
                        args.get("message_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'message_id'".to_string())
                            })?;
                    let body = args.get("body").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'body'".to_string())
                    })?;
                    let reply_all = args
                        .get("reply_all")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let output = run_applescript_output(&script_reply_to_message(
                        message_id, body, reply_all,
                    ))
                    .await?;
                    let result = DraftResult {
                        success: true,
                        message: output,
                    };
                    structured_result_with_text(&result, None)
                }

                "mark_read" => {
                    let message_id =
                        args.get("message_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'message_id'".to_string())
                            })?;
                    let read = args.get("read").and_then(|v| v.as_bool()).unwrap_or(true);

                    let output =
                        run_applescript_output(&script_mark_read(message_id, read)).await?;
                    structured_result_with_text(&json!({"success": true, "message": output}), None)
                }

                "mark_flagged" => {
                    let message_id =
                        args.get("message_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'message_id'".to_string())
                            })?;
                    let flagged = args
                        .get("flagged")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);

                    let output =
                        run_applescript_output(&script_mark_flagged(message_id, flagged)).await?;
                    structured_result_with_text(&json!({"success": true, "message": output}), None)
                }

                "move_message" => {
                    let message_id =
                        args.get("message_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'message_id'".to_string())
                            })?;
                    let target_mailbox = args
                        .get("target_mailbox")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("Missing 'target_mailbox'".to_string())
                        })?;
                    let target_account = args.get("target_account").and_then(|v| v.as_str());

                    let output = run_applescript_output(&script_move_message(
                        message_id,
                        target_mailbox,
                        target_account,
                    ))
                    .await?;
                    structured_result_with_text(&json!({"success": true, "message": output}), None)
                }

                "delete_message" => {
                    let message_id =
                        args.get("message_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'message_id'".to_string())
                            })?;

                    let output = run_applescript_output(&script_delete_message(message_id)).await?;
                    structured_result_with_text(&json!({"success": true, "message": output}), None)
                }

                _ => Err(ConnectorError::ToolNotFound),
            }
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
        Err(ConnectorError::ResourceNotFound)
    }
}
