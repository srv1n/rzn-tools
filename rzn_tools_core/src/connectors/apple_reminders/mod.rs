// Apple Reminders Connector - Native Reminders.app integration via AppleScript
// macOS only - manage reminders synced with iCloud
//
// Full CRUD support for reminders including:
// - Lists (folders)
// - Tasks with due dates, priorities, notes
// - Completion status

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

/// Apple Reminders connector - interact with Reminders.app via AppleScript
#[derive(Default)]
pub struct AppleRemindersConnector;

impl AppleRemindersConnector {
    pub fn new() -> Self {
        Self {}
    }
}

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct ReminderList {
    /// List name
    name: String,
    /// List ID
    id: String,
    /// Number of incomplete reminders
    incomplete_count: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct Reminder {
    /// Reminder ID (use for updates)
    id: String,
    /// Reminder name/title
    name: String,
    /// Body/notes
    body: Option<String>,
    /// Whether completed
    completed: bool,
    /// Completion date (if completed)
    completion_date: Option<String>,
    /// Due date (if set)
    due_date: Option<String>,
    /// Priority (0=none, 1=high, 5=medium, 9=low)
    priority: i32,
    /// Containing list name
    list: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateReminderResult {
    success: bool,
    reminder_id: Option<String>,
    message: String,
}

// ============================================================================
// AppleScript Generators
// ============================================================================

#[cfg(target_os = "macos")]
fn script_list_lists() -> String {
    r#"
tell application "Reminders"
    set output to ""
    repeat with l in lists
        set lName to name of l
        set lId to id of l
        set incompleteCount to count of (reminders of l whose completed is false)
        if output is not "" then set output to output & "|||"
        set output to output & lName & ":::" & lId & ":::" & incompleteCount
    end repeat
    return output
end tell
"#
    .to_string()
}

#[cfg(target_os = "macos")]
fn script_list_reminders(list_name: Option<&str>, show_completed: bool, limit: usize) -> String {
    let scope = match list_name {
        Some(name) => format!(r#"reminders of list "{}""#, escape_applescript_string(name)),
        None => "reminders".to_string(),
    };

    let filter = if show_completed {
        ""
    } else {
        " whose completed is false"
    };

    format!(
        r#"
tell application "Reminders"
    set allReminders to ({scope}{filter})
    set reminderCount to count of allReminders
    set maxCount to {limit}
    if reminderCount < maxCount then set maxCount to reminderCount

    set output to ""
    repeat with i from 1 to maxCount
        set r to item i of allReminders
        set rId to id of r
        set rName to name of r
        set rBody to body of r
        if rBody is missing value then set rBody to ""
        set rCompleted to completed of r
        set rCompletionDate to ""
        if rCompleted then
            try
                set rCompletionDate to completion date of r as string
            end try
        end if
        set rDueDate to ""
        try
            set rDueDate to due date of r as string
        end try
        set rPriority to priority of r
        set rList to name of container of r

        if output is not "" then set output to output & "|||"
        set output to output & rId & ":::" & rName & ":::" & rBody & ":::" & rCompleted & ":::" & rCompletionDate & ":::" & rDueDate & ":::" & rPriority & ":::" & rList
    end repeat
    return output
end tell
"#,
        scope = scope,
        filter = filter,
        limit = limit
    )
}

#[cfg(target_os = "macos")]
fn script_get_reminder(reminder_id: &str) -> String {
    format!(
        r#"
tell application "Reminders"
    set r to reminder id "{}"
    set rId to id of r
    set rName to name of r
    set rBody to body of r
    if rBody is missing value then set rBody to ""
    set rCompleted to completed of r
    set rCompletionDate to ""
    if rCompleted then
        try
            set rCompletionDate to completion date of r as string
        end try
    end if
    set rDueDate to ""
    try
        set rDueDate to due date of r as string
    end try
    set rPriority to priority of r
    set rList to name of container of r

    return rId & ":::" & rName & ":::" & rBody & ":::" & rCompleted & ":::" & rCompletionDate & ":::" & rDueDate & ":::" & rPriority & ":::" & rList
end tell
"#,
        escape_applescript_string(reminder_id)
    )
}

#[cfg(target_os = "macos")]
fn script_create_reminder(
    name: &str,
    list_name: Option<&str>,
    body: Option<&str>,
    due_date: Option<&str>,
    priority: Option<i32>,
) -> String {
    let list_clause = match list_name {
        Some(l) => format!(r#"in list "{}""#, escape_applescript_string(l)),
        None => "in default list".to_string(),
    };

    let mut props = vec![format!(r#"name:"{}""#, escape_applescript_string(name))];

    if let Some(b) = body {
        if !b.is_empty() {
            props.push(format!(r#"body:"{}""#, escape_applescript_string(b)));
        }
    }

    if let Some(p) = priority {
        props.push(format!("priority:{}", p));
    }

    let props_str = props.join(", ");

    // Due date needs special handling
    let due_clause = match due_date {
        Some(d) if !d.is_empty() => format!(
            r#"
    set due date of newReminder to date "{}""#,
            escape_applescript_string(d)
        ),
        _ => String::new(),
    };

    format!(
        r#"
tell application "Reminders"
    set newReminder to make new reminder {} with properties {{{}}}{}
    return id of newReminder
end tell
"#,
        list_clause, props_str, due_clause
    )
}

#[cfg(target_os = "macos")]
fn script_update_reminder(
    reminder_id: &str,
    name: Option<&str>,
    body: Option<&str>,
    due_date: Option<&str>,
    priority: Option<i32>,
) -> String {
    let mut updates = Vec::new();

    if let Some(n) = name {
        updates.push(format!(
            r#"set name of r to "{}""#,
            escape_applescript_string(n)
        ));
    }

    if let Some(b) = body {
        updates.push(format!(
            r#"set body of r to "{}""#,
            escape_applescript_string(b)
        ));
    }

    if let Some(d) = due_date {
        if d.is_empty() {
            updates.push("set due date of r to missing value".to_string());
        } else {
            updates.push(format!(
                r#"set due date of r to date "{}""#,
                escape_applescript_string(d)
            ));
        }
    }

    if let Some(p) = priority {
        updates.push(format!("set priority of r to {}", p));
    }

    let updates_str = updates.join("\n        ");

    format!(
        r#"
tell application "Reminders"
    set r to reminder id "{}"
    {}
    return "Reminder updated successfully"
end tell
"#,
        escape_applescript_string(reminder_id),
        updates_str
    )
}

#[cfg(target_os = "macos")]
fn script_complete_reminder(reminder_id: &str, completed: bool) -> String {
    format!(
        r#"
tell application "Reminders"
    set r to reminder id "{}"
    set completed of r to {}
    return "Reminder marked as {}"
end tell
"#,
        escape_applescript_string(reminder_id),
        if completed { "true" } else { "false" },
        if completed { "completed" } else { "incomplete" }
    )
}

#[cfg(target_os = "macos")]
fn script_delete_reminder(reminder_id: &str) -> String {
    format!(
        r#"
tell application "Reminders"
    delete reminder id "{}"
    return "Reminder deleted successfully"
end tell
"#,
        escape_applescript_string(reminder_id)
    )
}

#[cfg(target_os = "macos")]
fn script_create_list(name: &str) -> String {
    format!(
        r#"
tell application "Reminders"
    set newList to make new list with properties {{name:"{}"}}
    return id of newList
end tell
"#,
        escape_applescript_string(name)
    )
}

#[cfg(target_os = "macos")]
fn script_search_reminders(query: &str, include_completed: bool, limit: usize) -> String {
    let filter = if include_completed {
        ""
    } else {
        " and completed is false"
    };

    format!(
        r#"
tell application "Reminders"
    set searchTerm to "{query}"
    set allReminders to (reminders whose name contains searchTerm{filter})
    set reminderCount to count of allReminders
    set maxCount to {limit}
    if reminderCount < maxCount then set maxCount to reminderCount

    set output to ""
    repeat with i from 1 to maxCount
        set r to item i of allReminders
        set rId to id of r
        set rName to name of r
        set rBody to body of r
        if rBody is missing value then set rBody to ""
        set rCompleted to completed of r
        set rCompletionDate to ""
        set rDueDate to ""
        try
            set rDueDate to due date of r as string
        end try
        set rPriority to priority of r
        set rList to name of container of r

        if output is not "" then set output to output & "|||"
        set output to output & rId & ":::" & rName & ":::" & rBody & ":::" & rCompleted & ":::" & rCompletionDate & ":::" & rDueDate & ":::" & rPriority & ":::" & rList
    end repeat
    return output
end tell
"#,
        query = escape_applescript_string(query),
        filter = filter,
        limit = limit
    )
}

#[cfg(target_os = "macos")]
fn script_get_due_today() -> String {
    r#"
tell application "Reminders"
    set today to current date
    set todayStart to today - (time of today)
    set todayEnd to todayStart + 1 * days

    set output to ""
    repeat with r in (reminders whose completed is false and due date >= todayStart and due date < todayEnd)
        set rId to id of r
        set rName to name of r
        set rBody to body of r
        if rBody is missing value then set rBody to ""
        set rDueDate to due date of r as string
        set rPriority to priority of r
        set rList to name of container of r

        if output is not "" then set output to output & "|||"
        set output to output & rId & ":::" & rName & ":::" & rBody & ":::" & "false" & ":::" & "" & ":::" & rDueDate & ":::" & rPriority & ":::" & rList
    end repeat
    return output
end tell
"#
    .to_string()
}

#[cfg(target_os = "macos")]
fn script_get_overdue() -> String {
    r#"
tell application "Reminders"
    set now to current date

    set output to ""
    repeat with r in (reminders whose completed is false and due date < now)
        set rId to id of r
        set rName to name of r
        set rBody to body of r
        if rBody is missing value then set rBody to ""
        set rDueDate to due date of r as string
        set rPriority to priority of r
        set rList to name of container of r

        if output is not "" then set output to output & "|||"
        set output to output & rId & ":::" & rName & ":::" & rBody & ":::" & "false" & ":::" & "" & ":::" & rDueDate & ":::" & rPriority & ":::" & rList
    end repeat
    return output
end tell
"#
    .to_string()
}

// ============================================================================
// Parsing Functions
// ============================================================================

#[cfg(target_os = "macos")]
fn parse_lists(output: &str) -> Vec<ReminderList> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 3 {
                Some(ReminderList {
                    name: parts[0].to_string(),
                    id: parts[1].to_string(),
                    incomplete_count: parts[2].parse().unwrap_or(0),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_reminders(output: &str) -> Vec<Reminder> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 8 {
                Some(Reminder {
                    id: parts[0].to_string(),
                    name: parts[1].to_string(),
                    body: if parts[2].is_empty() {
                        None
                    } else {
                        Some(parts[2].to_string())
                    },
                    completed: parts[3] == "true",
                    completion_date: if parts[4].is_empty() {
                        None
                    } else {
                        Some(parts[4].to_string())
                    },
                    due_date: if parts[5].is_empty() {
                        None
                    } else {
                        Some(parts[5].to_string())
                    },
                    priority: parts[6].parse().unwrap_or(0),
                    list: parts[7].to_string(),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_single_reminder(output: &str) -> Option<Reminder> {
    let parts: Vec<&str> = output.split(":::").collect();
    if parts.len() >= 8 {
        Some(Reminder {
            id: parts[0].to_string(),
            name: parts[1].to_string(),
            body: if parts[2].is_empty() {
                None
            } else {
                Some(parts[2].to_string())
            },
            completed: parts[3] == "true",
            completion_date: if parts[4].is_empty() {
                None
            } else {
                Some(parts[4].to_string())
            },
            due_date: if parts[5].is_empty() {
                None
            } else {
                Some(parts[5].to_string())
            },
            priority: parts[6].parse().unwrap_or(0),
            list: parts[7].to_string(),
        })
    } else {
        None
    }
}

// ============================================================================
// Connector Implementation
// ============================================================================

#[async_trait]
impl crate::Connector for AppleRemindersConnector {
    fn name(&self) -> &'static str {
        "apple-reminders"
    }

    fn description(&self) -> &'static str {
        "Apple Reminders.app connector for macOS. Manage tasks and to-dos synced with iCloud. Create, complete, and organize reminders with due dates and priorities. Perfect for task management integration."
    }

    fn display_name(&self) -> &'static str {
        "Apple Reminders"
    }

    fn icon(&self) -> &'static str {
        "apple-reminders"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["tasks", "productivity", "personal"]
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
            let _ = run_applescript_output(r#"tell application "Reminders" to name"#).await?;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(ConnectorError::Other(
                "Apple Reminders is only available on macOS".to_string(),
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
                title: Some("Apple Reminders".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Native Reminders.app integration for task management. Syncs with iCloud. First use may trigger a permission prompt."
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
            // List Management
            Tool {
                name: Cow::Borrowed("list_lists"),
                title: Some("List Reminder Lists".to_string()),
                description: Some(Cow::Borrowed(
                    "List all reminder lists (folders). Returns list names, IDs, and incomplete reminder counts. Use list names when creating reminders.",
                )),
                input_schema: Arc::new(json!({"type": "object", "properties": {}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("create_list"),
                title: Some("Create List".to_string()),
                description: Some(Cow::Borrowed(
                    "Create a new reminder list. Returns the new list's ID.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Name for the new list. Required."
                            }
                        },
                        "required": ["name"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            // Reminder Listing
            Tool {
                name: Cow::Borrowed("list_reminders"),
                title: Some("List Reminders".to_string()),
                description: Some(Cow::Borrowed(
                    "List reminders with name, due date, priority, and completion status. Filter by list and/or completion status.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "list": {
                                "type": "string",
                                "description": "Filter to specific list name. If omitted, shows reminders from all lists."
                            },
                            "show_completed": {
                                "type": "boolean",
                                "description": "Include completed reminders. Default: false.",
                                "default": false
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum reminders to return. Default: 50.",
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
            Tool {
                name: Cow::Borrowed("get_reminder"),
                title: Some("Get Reminder Details".to_string()),
                description: Some(Cow::Borrowed(
                    "Get full details of a specific reminder by ID. Use IDs from list_reminders or search.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "reminder_id": {
                                "type": "string",
                                "description": "Reminder ID. Required."
                            }
                        },
                        "required": ["reminder_id"]
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
                name: Cow::Borrowed("get_due_today"),
                title: Some("Get Due Today".to_string()),
                description: Some(Cow::Borrowed(
                    "Get all incomplete reminders due today. Useful for daily task review.",
                )),
                input_schema: Arc::new(json!({"type": "object", "properties": {}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_overdue"),
                title: Some("Get Overdue".to_string()),
                description: Some(Cow::Borrowed(
                    "Get all incomplete reminders that are past their due date.",
                )),
                input_schema: Arc::new(json!({"type": "object", "properties": {}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            // Search
            Tool {
                name: Cow::Borrowed("search"),
                title: Some("Search Reminders".to_string()),
                description: Some(Cow::Borrowed(
                    "Search reminders by name. Optionally include completed reminders.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search term to find in reminder names. Required."
                            },
                            "include_completed": {
                                "type": "boolean",
                                "description": "Include completed reminders in results. Default: false.",
                                "default": false
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum results. Default: 20.",
                                "default": 20
                            }
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
            // Create & Update
            Tool {
                name: Cow::Borrowed("create_reminder"),
                title: Some("Create Reminder".to_string()),
                description: Some(Cow::Borrowed(
                    "Create a new reminder with optional due date, priority, and notes. Returns the new reminder's ID.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "Reminder title/name. Required."
                            },
                            "list": {
                                "type": "string",
                                "description": "List to add reminder to. Uses default list if omitted."
                            },
                            "body": {
                                "type": "string",
                                "description": "Additional notes for the reminder."
                            },
                            "due_date": {
                                "type": "string",
                                "description": "Due date in natural format (e.g., 'December 25, 2024 9:00 AM'). AppleScript date parsing applies."
                            },
                            "priority": {
                                "type": "integer",
                                "description": "Priority: 0=none, 1=high, 5=medium, 9=low.",
                                "enum": [0, 1, 5, 9]
                            }
                        },
                        "required": ["name"]
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
                name: Cow::Borrowed("update_reminder"),
                title: Some("Update Reminder".to_string()),
                description: Some(Cow::Borrowed(
                    "Update an existing reminder's name, body, due date, or priority. Only specified fields are updated.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "reminder_id": {
                                "type": "string",
                                "description": "Reminder ID to update. Required."
                            },
                            "name": {
                                "type": "string",
                                "description": "New name/title."
                            },
                            "body": {
                                "type": "string",
                                "description": "New notes/body text."
                            },
                            "due_date": {
                                "type": "string",
                                "description": "New due date. Use empty string to remove due date."
                            },
                            "priority": {
                                "type": "integer",
                                "description": "New priority: 0=none, 1=high, 5=medium, 9=low.",
                                "enum": [0, 1, 5, 9]
                            }
                        },
                        "required": ["reminder_id"]
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
                name: Cow::Borrowed("complete_reminder"),
                title: Some("Complete/Uncomplete Reminder".to_string()),
                description: Some(Cow::Borrowed(
                    "Mark a reminder as completed or incomplete.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "reminder_id": {
                                "type": "string",
                                "description": "Reminder ID. Required."
                            },
                            "completed": {
                                "type": "boolean",
                                "description": "True to mark complete, false to mark incomplete. Default: true.",
                                "default": true
                            }
                        },
                        "required": ["reminder_id"]
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
                name: Cow::Borrowed("delete_reminder"),
                title: Some("Delete Reminder".to_string()),
                description: Some(Cow::Borrowed(
                    "Permanently delete a reminder. Use with caution.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "reminder_id": {
                                "type": "string",
                                "description": "Reminder ID to delete. Required."
                            }
                        },
                        "required": ["reminder_id"]
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

        // Keep the surface small to reduce ambiguity and context bloat for agents.
        // Back-compat: non-listed tools are still accepted in call_tool().
        let tools = tools
            .into_iter()
            .filter(|t| {
                matches!(
                    t.name.as_ref(),
                    "list_lists"
                        | "list_reminders"
                        | "get_reminder"
                        | "search"
                        | "create_reminder"
                        | "update_reminder"
                        | "complete_reminder"
                )
            })
            .collect();

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
                "Apple Reminders is only available on macOS".to_string(),
            ));
        }

        #[cfg(target_os = "macos")]
        {
            let name = request.name.as_ref();
            let args = request.arguments.unwrap_or_default();

            match name {
                "list_lists" => {
                    let output = run_applescript_output(&script_list_lists()).await?;
                    let lists = parse_lists(&output);
                    structured_result_with_text(&lists, None)
                }

                "create_list" => {
                    let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'name'".to_string())
                    })?;

                    let output = run_applescript_output(&script_create_list(name)).await?;
                    structured_result_with_text(&json!({"success": true, "list_id": output}), None)
                }

                "list_reminders" => {
                    let list = args.get("list").and_then(|v| v.as_str());
                    let show_completed = args
                        .get("show_completed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

                    let output =
                        run_applescript_output(&script_list_reminders(list, show_completed, limit))
                            .await?;
                    let reminders = parse_reminders(&output);
                    structured_result_with_text(&reminders, None)
                }

                "get_reminder" => {
                    let reminder_id = args
                        .get("reminder_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("Missing 'reminder_id'".to_string())
                        })?;

                    let output = run_applescript_output(&script_get_reminder(reminder_id)).await?;
                    let reminder = parse_single_reminder(&output).ok_or_else(|| {
                        ConnectorError::Other("Failed to parse reminder".to_string())
                    })?;
                    structured_result_with_text(&reminder, None)
                }

                "get_due_today" => {
                    let output = run_applescript_output(&script_get_due_today()).await?;
                    let reminders = parse_reminders(&output);
                    structured_result_with_text(&reminders, None)
                }

                "get_overdue" => {
                    let output = run_applescript_output(&script_get_overdue()).await?;
                    let reminders = parse_reminders(&output);
                    structured_result_with_text(&reminders, None)
                }

                "search" => {
                    let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'query'".to_string())
                    })?;
                    let include_completed = args
                        .get("include_completed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

                    let output = run_applescript_output(&script_search_reminders(
                        query,
                        include_completed,
                        limit,
                    ))
                    .await?;
                    let reminders = parse_reminders(&output);
                    structured_result_with_text(&reminders, None)
                }

                "create_reminder" => {
                    let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'name'".to_string())
                    })?;
                    let list = args.get("list").and_then(|v| v.as_str());
                    let body = args.get("body").and_then(|v| v.as_str());
                    let due_date = args.get("due_date").and_then(|v| v.as_str());
                    let priority = args
                        .get("priority")
                        .and_then(|v| v.as_i64())
                        .map(|p| p as i32);

                    let output = run_applescript_output(&script_create_reminder(
                        name, list, body, due_date, priority,
                    ))
                    .await?;
                    let result = CreateReminderResult {
                        success: true,
                        reminder_id: Some(output),
                        message: "Reminder created successfully".to_string(),
                    };
                    structured_result_with_text(&result, None)
                }

                "update_reminder" => {
                    let reminder_id = args
                        .get("reminder_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("Missing 'reminder_id'".to_string())
                        })?;
                    let name = args.get("name").and_then(|v| v.as_str());
                    let body = args.get("body").and_then(|v| v.as_str());
                    let due_date = args.get("due_date").and_then(|v| v.as_str());
                    let priority = args
                        .get("priority")
                        .and_then(|v| v.as_i64())
                        .map(|p| p as i32);

                    let output = run_applescript_output(&script_update_reminder(
                        reminder_id,
                        name,
                        body,
                        due_date,
                        priority,
                    ))
                    .await?;
                    structured_result_with_text(&json!({"success": true, "message": output}), None)
                }

                "complete_reminder" => {
                    let reminder_id = args
                        .get("reminder_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("Missing 'reminder_id'".to_string())
                        })?;
                    let completed = args
                        .get("completed")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);

                    let output =
                        run_applescript_output(&script_complete_reminder(reminder_id, completed))
                            .await?;
                    structured_result_with_text(&json!({"success": true, "message": output}), None)
                }

                "delete_reminder" => {
                    let reminder_id = args
                        .get("reminder_id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ConnectorError::InvalidParams("Missing 'reminder_id'".to_string())
                        })?;

                    let output =
                        run_applescript_output(&script_delete_reminder(reminder_id)).await?;
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
