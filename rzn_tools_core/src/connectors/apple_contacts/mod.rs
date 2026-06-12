// Apple Contacts Connector - Native Contacts.app integration via AppleScript
// macOS only - access contacts from iCloud, local, Exchange, Google, etc.
//
// Provides search and lookup capabilities for contact information.
// Useful for looking up email addresses, phone numbers, and contact details.

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

/// Apple Contacts connector - interact with Contacts.app via AppleScript
#[derive(Default)]
pub struct AppleContactsConnector;

impl AppleContactsConnector {
    pub fn new() -> Self {
        Self {}
    }
}

// ============================================================================
// Data Structures
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct ContactGroup {
    /// Group name
    name: String,
    /// Group ID
    id: String,
    /// Number of contacts in group
    member_count: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct ContactSummary {
    /// Contact ID (use for get_contact)
    id: String,
    /// Full name
    name: String,
    /// First name
    first_name: Option<String>,
    /// Last name
    last_name: Option<String>,
    /// Organization/company
    organization: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Contact {
    /// Contact ID
    id: String,
    /// Full name
    name: String,
    /// First name
    first_name: Option<String>,
    /// Last name
    last_name: Option<String>,
    /// Organization/company
    organization: Option<String>,
    /// Job title
    job_title: Option<String>,
    /// Email addresses with labels
    emails: Vec<LabeledValue>,
    /// Phone numbers with labels
    phones: Vec<LabeledValue>,
    /// Addresses
    addresses: Vec<Address>,
    /// Notes
    note: Option<String>,
    /// Birthday (if set)
    birthday: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LabeledValue {
    label: String,
    value: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Address {
    label: String,
    street: Option<String>,
    city: Option<String>,
    state: Option<String>,
    zip: Option<String>,
    country: Option<String>,
}

// ============================================================================
// AppleScript Generators
// ============================================================================

#[cfg(target_os = "macos")]
fn script_list_groups() -> String {
    r#"
tell application "Contacts"
    set output to ""
    repeat with g in groups
        set gName to name of g
        set gId to id of g
        set memberCount to count of people of g
        if output is not "" then set output to output & "|||"
        set output to output & gName & ":::" & gId & ":::" & memberCount
    end repeat
    return output
end tell
"#
    .to_string()
}

#[cfg(target_os = "macos")]
fn script_list_contacts(group: Option<&str>, limit: usize) -> String {
    let scope = match group {
        Some(g) => format!(r#"people of group "{}""#, escape_applescript_string(g)),
        None => "people".to_string(),
    };

    format!(
        r#"
tell application "Contacts"
    set allPeople to {scope}
    set personCount to count of allPeople
    set maxCount to {limit}
    if personCount < maxCount then set maxCount to personCount

    set output to ""
    repeat with i from 1 to maxCount
        set p to item i of allPeople
        set pId to id of p
        set pName to name of p
        set pFirst to first name of p
        if pFirst is missing value then set pFirst to ""
        set pLast to last name of p
        if pLast is missing value then set pLast to ""
        set pOrg to organization of p
        if pOrg is missing value then set pOrg to ""

        if output is not "" then set output to output & "|||"
        set output to output & pId & ":::" & pName & ":::" & pFirst & ":::" & pLast & ":::" & pOrg
    end repeat
    return output
end tell
"#,
        scope = scope,
        limit = limit
    )
}

#[cfg(target_os = "macos")]
fn script_get_contact(contact_id: &str) -> String {
    format!(
        r#"
tell application "Contacts"
    set p to person id "{}"
    set pId to id of p
    set pName to name of p
    set pFirst to first name of p
    if pFirst is missing value then set pFirst to ""
    set pLast to last name of p
    if pLast is missing value then set pLast to ""
    set pOrg to organization of p
    if pOrg is missing value then set pOrg to ""
    set pTitle to job title of p
    if pTitle is missing value then set pTitle to ""
    set pNote to note of p
    if pNote is missing value then set pNote to ""
    set pBirthday to ""
    try
        set pBirthday to birth date of p as string
    end try

    -- Get emails
    set emailList to ""
    repeat with e in emails of p
        set eLabel to label of e
        set eValue to value of e
        if emailList is not "" then set emailList to emailList & ";;;"
        set emailList to emailList & eLabel & "=" & eValue
    end repeat

    -- Get phones
    set phoneList to ""
    repeat with ph in phones of p
        set phLabel to label of ph
        set phValue to value of ph
        if phoneList is not "" then set phoneList to phoneList & ";;;"
        set phoneList to phoneList & phLabel & "=" & phValue
    end repeat

    -- Get addresses (simplified)
    set addrList to ""
    repeat with a in addresses of p
        set aLabel to label of a
        set aStreet to street of a
        if aStreet is missing value then set aStreet to ""
        set aCity to city of a
        if aCity is missing value then set aCity to ""
        set aState to state of a
        if aState is missing value then set aState to ""
        set aZip to zip of a
        if aZip is missing value then set aZip to ""
        set aCountry to country of a
        if aCountry is missing value then set aCountry to ""
        if addrList is not "" then set addrList to addrList & ";;;"
        set addrList to addrList & aLabel & "=" & aStreet & "|" & aCity & "|" & aState & "|" & aZip & "|" & aCountry
    end repeat

    return pId & ":::" & pName & ":::" & pFirst & ":::" & pLast & ":::" & pOrg & ":::" & pTitle & ":::" & pNote & ":::" & pBirthday & "|||EMAILS|||" & emailList & "|||PHONES|||" & phoneList & "|||ADDRESSES|||" & addrList
end tell
"#,
        escape_applescript_string(contact_id)
    )
}

#[cfg(target_os = "macos")]
fn script_search_contacts(query: &str, limit: usize) -> String {
    format!(
        r#"
tell application "Contacts"
    set searchTerm to "{query}"
    set foundPeople to (people whose name contains searchTerm or organization contains searchTerm)
    set personCount to count of foundPeople
    set maxCount to {limit}
    if personCount < maxCount then set maxCount to personCount

    set output to ""
    repeat with i from 1 to maxCount
        set p to item i of foundPeople
        set pId to id of p
        set pName to name of p
        set pFirst to first name of p
        if pFirst is missing value then set pFirst to ""
        set pLast to last name of p
        if pLast is missing value then set pLast to ""
        set pOrg to organization of p
        if pOrg is missing value then set pOrg to ""

        if output is not "" then set output to output & "|||"
        set output to output & pId & ":::" & pName & ":::" & pFirst & ":::" & pLast & ":::" & pOrg
    end repeat
    return output
end tell
"#,
        query = escape_applescript_string(query),
        limit = limit
    )
}

#[cfg(target_os = "macos")]
fn script_search_by_email(email: &str) -> String {
    format!(
        r#"
tell application "Contacts"
    set output to ""
    repeat with p in people
        repeat with e in emails of p
            if value of e contains "{}" then
                set pId to id of p
                set pName to name of p
                set pFirst to first name of p
                if pFirst is missing value then set pFirst to ""
                set pLast to last name of p
                if pLast is missing value then set pLast to ""
                set pOrg to organization of p
                if pOrg is missing value then set pOrg to ""

                if output is not "" then set output to output & "|||"
                set output to output & pId & ":::" & pName & ":::" & pFirst & ":::" & pLast & ":::" & pOrg
                exit repeat
            end if
        end repeat
    end repeat
    return output
end tell
"#,
        escape_applescript_string(email)
    )
}

#[cfg(target_os = "macos")]
fn script_search_by_phone(phone: &str) -> String {
    // Normalize phone for search (remove common formatting)
    let normalized = phone
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect::<String>();

    format!(
        r#"
tell application "Contacts"
    set output to ""
    repeat with p in people
        repeat with ph in phones of p
            set phoneVal to value of ph
            -- Simple contains check
            if phoneVal contains "{}" then
                set pId to id of p
                set pName to name of p
                set pFirst to first name of p
                if pFirst is missing value then set pFirst to ""
                set pLast to last name of p
                if pLast is missing value then set pLast to ""
                set pOrg to organization of p
                if pOrg is missing value then set pOrg to ""

                if output is not "" then set output to output & "|||"
                set output to output & pId & ":::" & pName & ":::" & pFirst & ":::" & pLast & ":::" & pOrg
                exit repeat
            end if
        end repeat
    end repeat
    return output
end tell
"#,
        escape_applescript_string(&normalized)
    )
}

#[cfg(target_os = "macos")]
fn script_get_contact_count() -> String {
    r#"
tell application "Contacts"
    return count of people
end tell
"#
    .to_string()
}

// ============================================================================
// Parsing Functions
// ============================================================================

#[cfg(target_os = "macos")]
fn parse_groups(output: &str) -> Vec<ContactGroup> {
    output
        .split("|||")
        .filter(|s| !s.is_empty())
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 3 {
                Some(ContactGroup {
                    name: parts[0].to_string(),
                    id: parts[1].to_string(),
                    member_count: parts[2].parse().unwrap_or(0),
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_contact_summaries(output: &str) -> Vec<ContactSummary> {
    output
        .split("|||")
        .filter(|s| !s.is_empty() && !s.contains("|||EMAILS|||"))
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.split(":::").collect();
            if parts.len() >= 5 {
                Some(ContactSummary {
                    id: parts[0].to_string(),
                    name: parts[1].to_string(),
                    first_name: if parts[2].is_empty() {
                        None
                    } else {
                        Some(parts[2].to_string())
                    },
                    last_name: if parts[3].is_empty() {
                        None
                    } else {
                        Some(parts[3].to_string())
                    },
                    organization: if parts[4].is_empty() {
                        None
                    } else {
                        Some(parts[4].to_string())
                    },
                })
            } else {
                None
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn parse_full_contact(output: &str) -> Option<Contact> {
    // Split by major sections
    let sections: Vec<&str> = output.split("|||").collect();
    if sections.len() < 4 {
        return None;
    }

    // Parse basic info
    let basic_parts: Vec<&str> = sections[0].split(":::").collect();
    if basic_parts.len() < 8 {
        return None;
    }

    // Parse emails (section after EMAILS|||)
    let emails_section = sections
        .iter()
        .find(|s| s.starts_with("EMAILS|||"))
        .map(|s| &s[9..])
        .or_else(|| {
            sections
                .get(1)
                .filter(|s| !s.starts_with("PHONES") && !s.starts_with("ADDRESSES"))
                .copied()
        })
        .unwrap_or("");

    let emails: Vec<LabeledValue> = if emails_section.is_empty() {
        Vec::new()
    } else {
        emails_section
            .split(";;;")
            .filter_map(|e| {
                let parts: Vec<&str> = e.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some(LabeledValue {
                        label: parts[0].to_string(),
                        value: parts[1].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect()
    };

    // Parse phones
    let phones_idx = sections.iter().position(|s| s.starts_with("PHONES|||"));
    let phones_section = phones_idx
        .and_then(|i| sections.get(i))
        .map(|s| s.strip_prefix("PHONES|||").unwrap_or(s))
        .unwrap_or("");

    let phones: Vec<LabeledValue> = if phones_section.is_empty() {
        Vec::new()
    } else {
        phones_section
            .split(";;;")
            .filter_map(|p| {
                let parts: Vec<&str> = p.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some(LabeledValue {
                        label: parts[0].to_string(),
                        value: parts[1].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect()
    };

    // Parse addresses
    let addrs_idx = sections.iter().position(|s| s.starts_with("ADDRESSES|||"));
    let addrs_section = addrs_idx
        .and_then(|i| sections.get(i))
        .map(|s| s.strip_prefix("ADDRESSES|||").unwrap_or(s))
        .unwrap_or("");

    let addresses: Vec<Address> = if addrs_section.is_empty() {
        Vec::new()
    } else {
        addrs_section
            .split(";;;")
            .filter_map(|a| {
                let parts: Vec<&str> = a.splitn(2, '=').collect();
                if parts.len() == 2 {
                    let addr_parts: Vec<&str> = parts[1].split('|').collect();
                    Some(Address {
                        label: parts[0].to_string(),
                        street: addr_parts
                            .first()
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                        city: addr_parts
                            .get(1)
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                        state: addr_parts
                            .get(2)
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                        zip: addr_parts
                            .get(3)
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                        country: addr_parts
                            .get(4)
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string()),
                    })
                } else {
                    None
                }
            })
            .collect()
    };

    Some(Contact {
        id: basic_parts[0].to_string(),
        name: basic_parts[1].to_string(),
        first_name: if basic_parts[2].is_empty() {
            None
        } else {
            Some(basic_parts[2].to_string())
        },
        last_name: if basic_parts[3].is_empty() {
            None
        } else {
            Some(basic_parts[3].to_string())
        },
        organization: if basic_parts[4].is_empty() {
            None
        } else {
            Some(basic_parts[4].to_string())
        },
        job_title: if basic_parts[5].is_empty() {
            None
        } else {
            Some(basic_parts[5].to_string())
        },
        note: if basic_parts[6].is_empty() {
            None
        } else {
            Some(basic_parts[6].to_string())
        },
        birthday: if basic_parts[7].is_empty() {
            None
        } else {
            Some(basic_parts[7].to_string())
        },
        emails,
        phones,
        addresses,
    })
}

// ============================================================================
// Connector Implementation
// ============================================================================

#[async_trait]
impl crate::Connector for AppleContactsConnector {
    fn name(&self) -> &'static str {
        "apple-contacts"
    }

    fn description(&self) -> &'static str {
        "Apple Contacts.app connector for macOS. Search and lookup contacts from all configured accounts (iCloud, Google, Exchange, local). Find email addresses, phone numbers, and contact details."
    }

    fn display_name(&self) -> &'static str {
        "Apple Contacts"
    }

    fn icon(&self) -> &'static str {
        "apple-contacts"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["contacts", "productivity", "personal"]
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
            let _ = run_applescript_output(r#"tell application "Contacts" to name"#).await?;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(ConnectorError::Other(
                "Apple Contacts is only available on macOS".to_string(),
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
                title: Some("Apple Contacts".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Native Contacts.app integration. Access contacts from all configured accounts. First use may trigger a permission prompt."
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
            // Group Management
            Tool {
                name: Cow::Borrowed("list_groups"),
                title: Some("List Contact Groups".to_string()),
                description: Some(Cow::Borrowed(
                    "List all contact groups with names, IDs, and member counts. Use group names to filter contact listings.",
                )),
                input_schema: Arc::new(json!({"type": "object", "properties": {}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_contact_count"),
                title: Some("Get Contact Count".to_string()),
                description: Some(Cow::Borrowed(
                    "Get the total number of contacts in your address book.",
                )),
                input_schema: Arc::new(json!({"type": "object", "properties": {}}).as_object().unwrap().clone()),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            // Contact Listing
            Tool {
                name: Cow::Borrowed("list_contacts"),
                title: Some("List Contacts".to_string()),
                description: Some(Cow::Borrowed(
                    "List contacts with names and organizations. Optionally filter by group. Use get_contact for full details including emails and phones.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "group": {
                                "type": "string",
                                "description": "Filter to specific group name."
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum contacts to return. Default: 50.",
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
                name: Cow::Borrowed("get_contact"),
                title: Some("Get Contact Details".to_string()),
                description: Some(Cow::Borrowed(
                    "Get full contact details including all email addresses, phone numbers, addresses, notes, and birthday. Use contact IDs from list_contacts or search.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "contact_id": {
                                "type": "string",
                                "description": "Contact ID. Required."
                            }
                        },
                        "required": ["contact_id"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            // Search
            Tool {
                name: Cow::Borrowed("search"),
                title: Some("Search Contacts".to_string()),
                description: Some(Cow::Borrowed(
                    "Search contacts by name or organization. Returns matching contact summaries. Use get_contact for full details.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search term to find in contact name or organization. Required."
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
            Tool {
                name: Cow::Borrowed("search_by_email"),
                title: Some("Search by Email".to_string()),
                description: Some(Cow::Borrowed(
                    "Find contacts by email address. Useful for identifying senders. Returns matching contacts.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "email": {
                                "type": "string",
                                "description": "Email address or partial email to search for. Required."
                            }
                        },
                        "required": ["email"]
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
                name: Cow::Borrowed("search_by_phone"),
                title: Some("Search by Phone".to_string()),
                description: Some(Cow::Borrowed(
                    "Find contacts by phone number. Numbers are normalized for matching (ignores formatting). Returns matching contacts.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "phone": {
                                "type": "string",
                                "description": "Phone number to search for. Can include formatting. Required."
                            }
                        },
                        "required": ["phone"]
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
            .filter(|t| matches!(t.name.as_ref(), "list_contacts" | "get_contact" | "search"))
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
                "Apple Contacts is only available on macOS".to_string(),
            ));
        }

        #[cfg(target_os = "macos")]
        {
            let name = request.name.as_ref();
            let args = request.arguments.unwrap_or_default();

            match name {
                "list_groups" => {
                    let output = run_applescript_output(&script_list_groups()).await?;
                    let groups = parse_groups(&output);
                    structured_result_with_text(&groups, None)
                }

                "get_contact_count" => {
                    let output = run_applescript_output(&script_get_contact_count()).await?;
                    let count: i32 = output.trim().parse().unwrap_or(0);
                    structured_result_with_text(&json!({"total_contacts": count}), None)
                }

                "list_contacts" => {
                    let group = args.get("group").and_then(|v| v.as_str());
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

                    let output =
                        run_applescript_output(&script_list_contacts(group, limit)).await?;
                    let contacts = parse_contact_summaries(&output);
                    structured_result_with_text(&contacts, None)
                }

                "get_contact" => {
                    let contact_id =
                        args.get("contact_id")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| {
                                ConnectorError::InvalidParams("Missing 'contact_id'".to_string())
                            })?;

                    let output = run_applescript_output(&script_get_contact(contact_id)).await?;
                    let contact = parse_full_contact(&output).ok_or_else(|| {
                        ConnectorError::Other("Failed to parse contact".to_string())
                    })?;
                    structured_result_with_text(&contact, None)
                }

                "search" => {
                    let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'query'".to_string())
                    })?;
                    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

                    let output =
                        run_applescript_output(&script_search_contacts(query, limit)).await?;
                    let contacts = parse_contact_summaries(&output);
                    structured_result_with_text(&contacts, None)
                }

                "search_by_email" => {
                    let email = args.get("email").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'email'".to_string())
                    })?;

                    let output = run_applescript_output(&script_search_by_email(email)).await?;
                    let contacts = parse_contact_summaries(&output);
                    structured_result_with_text(&contacts, None)
                }

                "search_by_phone" => {
                    let phone = args.get("phone").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'phone'".to_string())
                    })?;

                    let output = run_applescript_output(&script_search_by_phone(phone)).await?;
                    let contacts = parse_contact_summaries(&output);
                    structured_result_with_text(&contacts, None)
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
