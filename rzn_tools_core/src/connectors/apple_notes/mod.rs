// Apple Notes Connector - Native Notes.app integration via AppleScript
// macOS only - access notes stored in iCloud, On My Mac, or other accounts
//
// This connector provides read/write access to Apple Notes without APIs.
// Great for personal knowledge bases, quick capture, and note organization.

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

/// Apple Notes connector - interact with Notes.app via AppleScript
#[derive(Default)]
pub struct AppleNotesConnector;

impl AppleNotesConnector {
    pub fn new() -> Self {
        Self {}
    }
}

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct NotesAccount {
    /// Account name (e.g., "iCloud", "On My Mac")
    name: String,
    /// Account ID
    id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct NotesFolder {
    /// Folder name
    name: String,
    /// Folder ID
    id: String,
    /// Parent account name
    account: String,
    /// Number of notes in folder
    note_count: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct NoteSummary {
    /// Note ID (use for get/update/delete operations)
    id: String,
    /// Note title (first line or name)
    name: String,
    /// Creation date
    creation_date: String,
    /// Last modification date
    modification_date: String,
    /// Containing folder name
    folder: String,
    /// Account name
    account: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct NoteContent {
    /// Note ID
    id: String,
    /// Note title/name
    name: String,
    /// Full note body as plain text
    body: String,
    /// Creation date
    creation_date: String,
    /// Last modification date
    modification_date: String,
    /// Containing folder
    folder: String,
    /// Account name
    account: String,
    /// Whether body was truncated
    truncated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateNoteResult {
    success: bool,
    note_id: Option<String>,
    message: String,
}

// ============================================================================
// AppleScript Generators
// ============================================================================

#[cfg(target_os = "macos")]
fn script_list_accounts() -> String {
    r#"
tell application "Notes"
    set output to ""
    repeat with acc in accounts
        set accName to name of acc
        set accId to id of acc
        if output is not "" then set output to output & "|||"
        set output to output & accName & ":::" & accId
    end repeat
    return output
end tell
"#
    .to_string()
}

#[cfg(target_os = "macos")]
fn script_list_folders(account: Option<&str>) -> String {
    match account {
        Some(acc) => format!(
            r#"
tell application "Notes"
    set output to ""
    set acc to account "{}"
    repeat with f in folders of acc
        set fName to name of f
        set fId to id of f
        set noteCount to count of notes of f
        if output is not "" then set output to output & "|||"
        set output to output & fName & ":::" & fId & ":::" & (name of acc) & ":::" & noteCount
    end repeat
    return output
end tell
"#,
            escape_applescript_string(acc)
        ),
        None => r#"
tell application "Notes"
    set output to ""
    repeat with acc in accounts
        repeat with f in folders of acc
            set fName to name of f
            set fId to id of f
            set noteCount to count of notes of f
            if output is not "" then set output to output & "|||"
            set output to output & fName & ":::" & fId & ":::" & (name of acc) & ":::" & noteCount
        end repeat
    end repeat
    return output
end tell
"#
        .to_string(),
    }
}

#[cfg(target_os = "macos")]
fn script_list_notes(folder: Option<&str>, account: Option<&str>, limit: usize) -> String {
    let scope = match (folder, account) {
        (Some(f), Some(a)) => format!(
            r#"notes of folder "{}" of account "{}""#,
            escape_applescript_string(f),
            escape_applescript_string(a)
        ),
        (Some(f), None) => format!(r#"notes of folder "{}""#, escape_applescript_string(f)),
        (None, Some(a)) => format!(r#"notes of account "{}""#, escape_applescript_string(a)),
        (None, None) => "notes".to_string(),
    };

    format!(
        r#"
tell application "Notes"
    set allNotes to {scope}
    set noteCount to count of allNotes
    set maxCount to {limit}
    if noteCount < maxCount then set maxCount to noteCount

    set output to ""
    repeat with i from 1 to maxCount
        set n to item i of allNotes
        set nId to id of n
        set nName to name of n
        set nCreated to creation date of n as string
        set nModified to modification date of n as string

        -- Get folder and account info (with error handling)
        set nFolder to ""
        set nAccount to ""
        try
            set nFolder to name of container of n
        end try
        try
            set nAccount to name of account of container of n
        end try

        if output is not "" then set output to output & "|||"
        set output to output & nId & ":::" & nName & ":::" & nCreated & ":::" & nModified & ":::" & nFolder & ":::" & nAccount
    end repeat
    return output
end tell
"#,
        scope = scope,
        limit = limit
    )
}

#[cfg(target_os = "macos")]
fn script_get_note(note_id: &str) -> String {
    format!(
        r#"
tell application "Notes"
    set n to note id "{}"
    set nId to id of n
    set nName to name of n
    set nBody to plaintext of n
    set nCreated to creation date of n as string
    set nModified to modification date of n as string
    set nFolder to ""
    set nAccount to ""
    try
        set nFolder to name of container of n
    end try
    try
        set nAccount to name of account of container of n
    end try

    return nId & ":::" & nName & ":::" & nCreated & ":::" & nModified & ":::" & nFolder & ":::" & nAccount & "|||BODY|||" & nBody
end tell
"#,
        escape_applescript_string(note_id)
    )
}

#[cfg(target_os = "macos")]
fn script_search_notes(
    query: &str,
    folder: Option<&str>,
    account: Option<&str>,
    limit: usize,
) -> String {
    let scope = match (folder, account) {
        (Some(f), Some(a)) => format!(
            r#"notes of folder "{}" of account "{}""#,
            escape_applescript_string(f),
            escape_applescript_string(a)
        ),
        (Some(f), None) => format!(r#"notes of folder "{}""#, escape_applescript_string(f)),
        (None, Some(a)) => format!(r#"notes of account "{}""#, escape_applescript_string(a)),
        (None, None) => "notes".to_string(),
    };

    format!(
        r#"
tell application "Notes"
    set searchTerm to "{query}"
    set allNotes to {scope}
    set foundNotes to {{}}

    repeat with n in allNotes
        set nName to name of n
        set nBody to plaintext of n
        if nName contains searchTerm or nBody contains searchTerm then
            set end of foundNotes to n
        end if
        if (count of foundNotes) >= {limit} then exit repeat
    end repeat

    set output to ""
    repeat with n in foundNotes
        set nId to id of n
        set nName to name of n
        set nCreated to creation date of n as string
        set nModified to modification date of n as string
        set nFolder to ""
        set nAccount to ""
        try
            set nFolder to name of container of n
        end try
        try
            set nAccount to name of account of container of n
        end try

        if output is not "" then set output to output & "|||"
        set output to output & nId & ":::" & nName & ":::" & nCreated & ":::" & nModified & ":::" & nFolder & ":::" & nAccount
    end repeat
    return output
end tell
"#,
        query = escape_applescript_string(query),
        scope = scope,
        limit = limit
    )
}

#[cfg(target_os = "macos")]
fn script_create_note(
    title: &str,
    body: &str,
    folder: Option<&str>,
    account: Option<&str>,
) -> String {
    let location = match (folder, account) {
        (Some(f), Some(a)) => format!(
            r#"in folder "{}" of account "{}""#,
            escape_applescript_string(f),
            escape_applescript_string(a)
        ),
        (Some(f), None) => format!(r#"in folder "{}""#, escape_applescript_string(f)),
        (None, Some(a)) => format!(
            r#"in default folder of account "{}""#,
            escape_applescript_string(a)
        ),
        (None, None) => String::new(),
    };

    // Notes uses HTML-like body format
    let full_body = format!(
        "<h1>{}</h1><br>{}",
        escape_applescript_string(title),
        escape_applescript_string(body).replace('\n', "<br>")
    );

    format!(
        r#"
tell application "Notes"
    set newNote to make new note {} with properties {{body:"{}"}}
    return id of newNote
end tell
"#,
        location, full_body
    )
}

#[cfg(target_os = "macos")]
fn script_update_note(note_id: &str, body: &str) -> String {
    let html_body = escape_applescript_string(body).replace('\n', "<br>");
    format!(
        r#"
tell application "Notes"
    set n to note id "{}"
    set body of n to "{}"
    return "Note updated successfully"
end tell
"#,
        escape_applescript_string(note_id),
        html_body
    )
}

#[cfg(target_os = "macos")]
fn script_append_to_note(note_id: &str, text: &str) -> String {
    let html_text = escape_applescript_string(text).replace('\n', "<br>");
    format!(
        r#"
tell application "Notes"
    set n to note id "{}"
    set currentBody to body of n
    set body of n to currentBody & "<br>" & "{}"
    return "Text appended successfully"
end tell
"#,
        escape_applescript_string(note_id),
        html_text
    )
}

#[cfg(target_os = "macos")]
fn script_delete_note(note_id: &str) -> String {
    format!(
        r#"
tell application "Notes"
    delete note id "{}"
    return "Note deleted successfully"
end tell
"#,
        escape_applescript_string(note_id)
    )
}

#[cfg(target_os = "macos")]
fn script_create_folder(name: &str, account: Option<&str>) -> String {
    let location = match account {
        Some(a) => format!(r#"in account "{}""#, escape_applescript_string(a)),
        None => String::new(),
    };

    format!(
        r#"
tell application "Notes"
    set newFolder to make new folder {} with properties {{name:"{}"}}
    return id of newFolder
end tell
"#,
        location,
        escape_applescript_string(name)
    )
}

// ============================================================================
// Parsing Functions
// ============================================================================

#[cfg(target_os = "macos")]
fn parse_accounts(output: &str) -> Vec<NotesAccount> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 2 {
                Some(NotesAccount {
                    name: parts[0].to_string(),
                    id: parts[1].to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_folders(output: &str) -> Vec<NotesFolder> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 4 {
                Some(NotesFolder {
                    name: parts[0].to_string(),
                    id: parts[1].to_string(),
                    account: parts[2].to_string(),
                    note_count: parts[3].parse().unwrap_or(0),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_note_summaries(output: &str) -> Vec<NoteSummary> {
    output
        .split("|||")
        .filter(|s| !s.is_empty() && !s.contains("|||BODY|||"))
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 6 {
                Some(NoteSummary {
                    id: parts[0].to_string(),
                    name: parts[1].to_string(),
                    creation_date: parts[2].to_string(),
                    modification_date: parts[3].to_string(),
                    folder: parts[4].to_string(),
                    account: parts[5].to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_note_content(output: &str, max_body_len: usize) -> Option<NoteContent> {
    let parts: Vec<&str> = output.split("|||BODY|||").collect();
    if parts.len() != 2 {
        return None;
    }

    let meta_parts: Vec<&str> = parts[0].split(":::").collect();
    if meta_parts.len() < 6 {
        return None;
    }

    let mut body = parts[1].to_string();
    let truncated = if body.len() > max_body_len {
        body.truncate(max_body_len);
        true
    } else {
        false
    };

    Some(NoteContent {
        id: meta_parts[0].to_string(),
        name: meta_parts[1].to_string(),
        creation_date: meta_parts[2].to_string(),
        modification_date: meta_parts[3].to_string(),
        folder: meta_parts[4].to_string(),
        account: meta_parts[5].to_string(),
        body,
        truncated,
    })
}

// ============================================================================
// Connector Implementation
// ============================================================================

#[async_trait]
impl crate::Connector for AppleNotesConnector {
    fn name(&self) -> &'static str {
        "apple-notes"
    }

    fn description(&self) -> &'static str {
        "Apple Notes.app connector for macOS. Access notes from iCloud, On My Mac, and other accounts. Create, read, search, and organize notes. Great for personal knowledge management."
    }

    fn display_name(&self) -> &'static str {
        "Apple Notes"
    }

    fn icon(&self) -> &'static str {
        "apple-notes"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["notes", "productivity", "personal"]
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
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        #[cfg(target_os = "macos")]
        {
            let _ = run_applescript_output(r#"tell application "Notes" to name"#).await?;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(ConnectorError::Other(
                "Apple Notes is only available on macOS".to_string(),
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
                title: Some("Apple Notes".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Native Notes.app integration. Access all notes from iCloud and local accounts. First use may trigger a permission prompt."
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
        // Back-compat: legacy tools are still accepted in call_tool().
        let tools = vec![
            Tool {
                name: Cow::Borrowed("list_notes"),
                title: Some("List Notes".to_string()),
                description: Some(Cow::Borrowed(
                    "List note summaries (requires explicit user permission). Use get_note for \
full content. Example: folder=\"Work\" limit=20.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "folder": { "type": "string", "description": "Optional folder filter." },
                            "account": { "type": "string", "description": "Optional account filter (e.g., iCloud)." },
                            "limit": { "type": "integer", "default": 50, "description": "Max notes (default 50, max 200)." }
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
                name: Cow::Borrowed("get_note"),
                title: Some("Get Note".to_string()),
                description: Some(Cow::Borrowed(
                    "Get a note by note_id (requires explicit user permission). Tip: set \
max_body_length to keep output small. Example: note_id=\"123\" max_body_length=12000.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "note_id": { "type": "string", "description": "Note ID from list_notes/search." },
                            "max_body_length": { "type": "integer", "default": 50000, "description": "Max characters of body." }
                        },
                        "required": ["note_id"]
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
                title: Some("Search Notes".to_string()),
                description: Some(Cow::Borrowed(
                    "Search notes by keyword (requires explicit user permission). Use to find \
note IDs, then call get_note. Example: query=\"meeting\" limit=10.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" },
                            "folder": { "type": "string" },
                            "account": { "type": "string" },
                            "limit": { "type": "integer", "default": 20 }
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
                name: Cow::Borrowed("create_note"),
                title: Some("Create Note".to_string()),
                description: Some(Cow::Borrowed(
                    "Create a new note (requires explicit user permission). Example: title=\"Todo\" body=\"- item\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "title": { "type": "string" },
                            "body": { "type": "string" },
                            "folder": { "type": "string" },
                            "account": { "type": "string" }
                        },
                        "required": ["title", "body"]
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
                name: Cow::Borrowed("update_note"),
                title: Some("Update Note".to_string()),
                description: Some(Cow::Borrowed(
                    "Replace a note body (requires explicit user permission). Tip: call \
get_note first if you need to preserve existing content.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "note_id": { "type": "string" },
                            "body": { "type": "string" }
                        },
                        "required": ["note_id", "body"]
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
                name: Cow::Borrowed("append_to_note"),
                title: Some("Append to Note".to_string()),
                description: Some(Cow::Borrowed(
                    "Append text to a note (requires explicit user permission). Example: note_id=\"123\" text=\"\\n- item\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "note_id": { "type": "string" },
                            "text": { "type": "string" }
                        },
                        "required": ["note_id", "text"]
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
                "Apple Notes is only available on macOS".to_string(),
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

                "list_folders" => {
                    let account = args.get("account").and_then(|v| v.as_str());
                    let output = run_applescript_output(&script_list_folders(account)).await?;
                    let folders = parse_folders(&output);
                    structured_result_with_text(&folders, None)
                }

                "create_folder" => {
                    let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'name'".to_string())
                    })?;
                    let account = args.get("account").and_then(|v| v.as_str());

                    let output =
                        run_applescript_output(&script_create_folder(name, account)).await?;
                    structured_result_with_text(
                        &json!({"success": true, "folder_id": output}),
                        None,
                    )
                }

                "list_notes" => {
                    let folder = args.get("folder").and_then(|v| v.as_str());
                    let account = args.get("account").and_then(|v| v.as_str());
                    let limit = args
                        .get("limit")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(50)
                        .min(200) as usize;

                    let output =
                        run_applescript_output(&script_list_notes(folder, account, limit)).await?;
                    let notes = parse_note_summaries(&output);
                    structured_result_with_text(&notes, None)
                }

                "get_note" => {
                    let note_id =
                        args.get("note_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'note_id'".to_string())
                            })?;
                    let max_len = args
                        .get("max_body_length")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(50000) as usize;

                    let output = run_applescript_output(&script_get_note(note_id)).await?;
                    let note = parse_note_content(&output, max_len)
                        .ok_or_else(|| ConnectorError::Other("Failed to parse note".to_string()))?;
                    structured_result_with_text(&note, None)
                }

                "search" => {
                    let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'query'".to_string())
                    })?;
                    let folder = args.get("folder").and_then(|v| v.as_str());
                    let account = args.get("account").and_then(|v| v.as_str());
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

                    let output =
                        run_applescript_output(&script_search_notes(query, folder, account, limit))
                            .await?;
                    let results = parse_note_summaries(&output);
                    structured_result_with_text(&results, None)
                }

                "create_note" => {
                    let title = args.get("title").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'title'".to_string())
                    })?;
                    let body = args.get("body").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'body'".to_string())
                    })?;
                    let folder = args.get("folder").and_then(|v| v.as_str());
                    let account = args.get("account").and_then(|v| v.as_str());

                    let output =
                        run_applescript_output(&script_create_note(title, body, folder, account))
                            .await?;
                    let result = CreateNoteResult {
                        success: true,
                        note_id: Some(output),
                        message: "Note created successfully".to_string(),
                    };
                    structured_result_with_text(&result, None)
                }

                "update_note" => {
                    let note_id =
                        args.get("note_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'note_id'".to_string())
                            })?;
                    let body = args.get("body").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'body'".to_string())
                    })?;

                    let output = run_applescript_output(&script_update_note(note_id, body)).await?;
                    structured_result_with_text(&json!({"success": true, "message": output}), None)
                }

                "append_to_note" => {
                    let note_id =
                        args.get("note_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'note_id'".to_string())
                            })?;
                    let text = args.get("text").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'text'".to_string())
                    })?;

                    let output =
                        run_applescript_output(&script_append_to_note(note_id, text)).await?;
                    structured_result_with_text(&json!({"success": true, "message": output}), None)
                }

                "delete_note" => {
                    let note_id =
                        args.get("note_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'note_id'".to_string())
                            })?;

                    let output = run_applescript_output(&script_delete_note(note_id)).await?;
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
