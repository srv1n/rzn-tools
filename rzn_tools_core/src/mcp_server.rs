use base64::Engine as _;
use once_cell::sync::Lazy;
use serde_json::{json, Map as JsonMap, Value};
use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::PathBuf,
    sync::Arc,
};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::{
    auth::AuthDetails,
    capabilities::{ConnectorConfigSchema, FieldType},
    display::from_normalized::{
        stash_original_structured_content_in_meta,
        try_convert_normalized_structured_content_to_display_v1,
    },
    flow_failure::build_tool_flow_failure_draft,
    utils::structured_result_with_text,
    ConnectorError, ProviderRegistry, URLPatternSpec,
};
use rmcp::model::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AuthState {
    authorized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    authorized_at: Option<String>,
}

/// Response for connectors/list endpoint.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListConnectorsResult {
    pub connectors: Vec<ConnectorMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    ApiKey,
    Oauth2,
    Config,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthSlotMetadata {
    pub slot_id: String,
    pub provider_id: String,
    pub auth_method_id: String,
    pub auth_kind: AuthKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<JsonMap<String, Value>>,
    /// True when the connector cannot operate without these credentials.
    /// False for optional-credential connectors (e.g. reddit) that still
    /// advertise an auth method so hosts can offer setup.
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectorMetadata {
    /// Stable connector id (same as `name`, but explicit for host UIs).
    pub connector_id: String,
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub icon: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    pub tools_count: usize,
    pub tools: Vec<String>,
    pub categories: Vec<String>,
    pub url_patterns: Vec<URLPatternSpec>,
    /// True if any tool can be used without credentials.
    pub supports_anonymous_read: bool,
    /// True if any tool requires credentials (even if some read tools don't).
    pub requires_auth: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auth_slots: Vec<AuthSlotMetadata>,
    pub auth_required: bool,
    pub auth_status: AuthStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListIngestSourcesResult {
    pub ingest_sources: Vec<IngestSource>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeToolAlias {
    alias_connector: &'static str,
    alias_tool: &'static str,
    target_connector: &'static str,
    target_tool: &'static str,
}

// Runtime aliases accepted by `call_tool` but intentionally NOT listed by
// `list_tools`. Per Anthropic's "Writing tools for agents" guidance,
// duplicate-surface tools (same behavior under multiple names) waste agent
// context and muddy tool selection. Aliases here exist for backwards
// compatibility of existing callers only.
//
// Note: simple connector renames like `youtube_transcripts` → `youtube`
// are handled by `ProviderRegistry.aliases` (see `register_alias`) and do
// not need an entry here. Only aliases that also rename the *tool* belong
// in this list.
fn runtime_tool_aliases(registry: &ProviderRegistry) -> Vec<RuntimeToolAlias> {
    let mut aliases = Vec::new();

    if registry.providers.contains_key("federated") {
        aliases.push(RuntimeToolAlias {
            alias_connector: "web_search",
            alias_tool: "search",
            target_connector: "federated",
            target_tool: "federated_search",
        });
    }

    if registry.providers.contains_key("web") {
        aliases.push(RuntimeToolAlias {
            alias_connector: "web_search",
            alias_tool: "get",
            target_connector: "web",
            target_tool: "get",
        });
    }

    aliases
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListIngestSourcesParams {
    /// Optional filter: only include these connectors.
    /// Empty means "all connectors".
    #[serde(default)]
    pub connectors: Vec<String>,

    /// Optional filter: only include these tool categories (from schema `_meta.category`).
    /// Empty means "default seed/schedule categories".
    #[serde(default)]
    pub categories: Vec<String>,

    /// Include `category="read"` tools that are suitable for ingestion scheduling
    /// (currently defined as: `supports_cursor=true`).
    #[serde(default = "default_true")]
    pub include_read: bool,

    /// Include one-shot fetch tools (typically `category="read"` with `supports_cursor=false`).
    /// These are excluded by default because they are not good scheduled ingestion "sources".
    #[serde(default)]
    pub include_fetch: bool,
}

impl Default for ListIngestSourcesParams {
    fn default() -> Self {
        Self {
            connectors: Vec::new(),
            categories: Vec::new(),
            include_read: true,
            include_fetch: false,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IngestSource {
    pub id: String,
    pub connector: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub tool: String,
    pub input_schema: JsonMap<String, Value>,
    #[serde(default, skip_serializing_if = "JsonMap::is_empty")]
    pub default_args: JsonMap<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub auth_required: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ListConnectorsParams {
    #[serde(default)]
    pub probe_auth: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthStatus {
    NotRequired,
    NeedsSetup,
    Ready,
    Invalid,
    Unknown,
}

/// MCP Server implementation that wraps the ProviderRegistry
pub struct McpServer {
    registry: Arc<Mutex<ProviderRegistry>>,
    auth_status: Arc<Mutex<std::collections::HashMap<String, AuthState>>>,
}

impl McpServer {
    pub fn new(registry: Arc<Mutex<ProviderRegistry>>) -> Self {
        Self {
            registry,
            auth_status: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Get aggregated capabilities from all connectors
    pub async fn get_capabilities(&self) -> ServerCapabilities {
        let registry = self.registry.lock().await;
        let mut capabilities = ServerCapabilities::default();

        // Check if any connector supports tools
        for (_name, connector) in registry.providers.iter() {
            let conn = connector.lock().await;
            let conn_caps = conn.capabilities().await;
            if conn_caps.tools.is_some() {
                capabilities.tools = conn_caps.tools;
            }
            if conn_caps.resources.is_some() {
                capabilities.resources = conn_caps.resources;
            }
            if conn_caps.prompts.is_some() {
                capabilities.prompts = conn_caps.prompts;
            }
        }

        capabilities
    }

    /// Handle initialize request
    pub async fn handle_initialize(
        &self,
        _request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        info!("MCP Server initializing");

        Ok(InitializeResult {
            protocol_version: ProtocolVersion::LATEST,
            capabilities: self.get_capabilities().await,
            server_info: Implementation {
                name: "rzn-tools".to_string(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some("Multi-connector data sourcing server supporting various data sources including academic papers, social media, search engines, and more.".to_string()),
        })
    }

    /// Handle list_resources request - aggregates from all connectors
    pub async fn handle_list_resources(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        let registry = self.registry.lock().await;
        let mut all_resources = Vec::new();

        // Collect resources from all connectors
        for (_name, connector) in registry.providers.iter() {
            let c = connector.lock().await;
            match c.list_resources(request.clone()).await {
                Ok(response) => {
                    all_resources.extend(response.resources);
                }
                Err(e) => {
                    error!("Error listing resources from connector: {:?}", e);
                }
            }
        }

        Ok(ListResourcesResult {
            resources: all_resources,
            next_cursor: None,
        })
    }

    /// Handle read_resource request - routes to appropriate connector
    pub async fn handle_read_resource(
        &self,
        request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        let registry = self.registry.lock().await;

        // Try each connector until one handles the resource
        for (_name, connector) in registry.providers.iter() {
            let c = connector.lock().await;
            match c.read_resource(request.clone()).await {
                Ok(contents) => return Ok(contents),
                Err(ConnectorError::ResourceNotFound) => continue,
                Err(e) => return Err(e),
            }
        }

        Err(ConnectorError::ResourceNotFound)
    }

    /// Handle list_tools request - aggregates from all connectors
    pub async fn handle_list_tools(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let registry = self.registry.lock().await;
        let mut all_tools = Vec::new();

        // Collect tools from all connectors
        for (connector_name, connector) in registry.providers.iter() {
            let c = connector.lock().await;
            let schema = c.config_schema();
            let connector_icon_url = icon_url_from_slug(c.icon());
            match c.list_tools(request.clone()).await {
                Ok(response) => {
                    let tools_require_auth = response
                        .tools
                        .iter()
                        .any(|tool| tool_requires_auth(tool) == Some(true));
                    let requires_auth = c.requires_auth() || tools_require_auth;
                    let inferred_slots =
                        infer_auth_slots(connector_name.as_str(), &**c, &schema, requires_auth);
                    let default_slot_id = inferred_slots.first().map(|s| s.slot_id.clone());

                    // Prefix tool names with connector name to avoid conflicts
                    let prefixed_tools: Vec<Tool> = response
                        .tools
                        .into_iter()
                        .map(|mut tool| {
                            tool.name = format!("{}/{}", connector_name, tool.name).into();
                            // Best-effort: inject contract metadata into input_schema._meta for host UIs.
                            let mut schema_obj = tool.input_schema.as_ref().clone();
                            let meta_value = schema_obj
                                .entry("_meta".to_string())
                                .or_insert_with(|| Value::Object(JsonMap::new()));
                            if let Some(meta) = meta_value.as_object_mut() {
                                if meta.get("auth_required").is_none() {
                                    meta.insert("auth_required".to_string(), json!(requires_auth));
                                }

                                let auth_required =
                                    meta.get("auth_required").and_then(|v| v.as_bool());
                                if auth_required == Some(true)
                                    && meta.get("requires_slot_id").is_none()
                                {
                                    if let Some(slot_id) = default_slot_id.as_ref() {
                                        meta.insert("requires_slot_id".to_string(), json!(slot_id));
                                    }
                                }

                                if meta.get("operation").is_none() {
                                    let operation = meta
                                        .get("category")
                                        .and_then(|v| v.as_str())
                                        .map(|c| match c {
                                            "list" | "search" | "read" => "read",
                                            _ => "write",
                                        })
                                        .unwrap_or("read");
                                    meta.insert("operation".to_string(), json!(operation));
                                }
                            }
                            inject_tool_auth_meta(&mut schema_obj, &inferred_slots);
                            tool.input_schema = Arc::new(schema_obj);

                            if tool.icons.is_none() {
                                if let Some(src) = connector_icon_url.clone() {
                                    tool.icons = Some(vec![Icon {
                                        src,
                                        mime_type: Some("image/svg+xml".to_string()),
                                        sizes: Some(vec!["any".to_string()]),
                                    }]);
                                }
                            }
                            tool
                        })
                        .collect();
                    all_tools.extend(prefixed_tools);
                }
                Err(e) => {
                    error!(
                        "Error listing tools from connector {}: {:?}",
                        connector_name, e
                    );
                }
            }
        }

        // Intentionally do NOT expand runtime_tool_aliases into the tool
        // listing: duplicate surface confuses agents and wastes context
        // (see guidance in `.claude/skills/tool-design/SKILL.md`).
        // Aliases remain callable via `call_tool` for backwards compat.

        // Add generic auth tools per connector following MCP tool semantics
        for (connector_name, connector) in registry.providers.iter() {
            let c = connector.lock().await;
            let schema = c.config_schema();
            let connector_icon_url = icon_url_from_slug(c.icon());
            drop(c);

            // auth/<provider>/set
            let set_tool = Tool {
                name: format!("auth/{}/set", connector_name).into(),
                title: None,
                description: Some(format!(
                    "Set credentials for '{}' (tokens, OAuth results, or basic credentials) following MCP tool flow.",
                    connector_name
                ).into()),
                input_schema: Arc::new(config_schema_to_jsonschema(&schema)),
                output_schema: None,
                annotations: None,
                icons: connector_icon_url.clone().map(|src| {
                    vec![Icon {
                        src,
                        mime_type: Some("image/svg+xml".to_string()),
                        sizes: Some(vec!["any".to_string()]),
                    }]
                }),
            };
            all_tools.push(set_tool);

            // auth/<provider>/test
            let test_tool = Tool {
                name: format!("auth/{}/test", connector_name).into(),
                title: None,
                description: Some("Test authentication for the connector.".into()),
                input_schema: Arc::new(
                    serde_json::json!({"type":"object","properties":{}})
                        .as_object()
                        .expect("Schema must be an object")
                        .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: connector_icon_url.clone().map(|src| {
                    vec![Icon {
                        src,
                        mime_type: Some("image/svg+xml".to_string()),
                        sizes: Some(vec!["any".to_string()]),
                    }]
                }),
            };
            all_tools.push(test_tool);

            // auth/<provider>/get_schema
            let schema_tool = Tool {
                name: format!("auth/{}/get_schema", connector_name).into(),
                title: None,
                description: Some(
                    "Return JSON schema for connector credentials (fields/types).".into(),
                ),
                input_schema: Arc::new(
                    serde_json::json!({"type":"object","properties":{}})
                        .as_object()
                        .expect("Schema must be an object")
                        .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: connector_icon_url.clone().map(|src| {
                    vec![Icon {
                        src,
                        mime_type: Some("image/svg+xml".to_string()),
                        sizes: Some(vec!["any".to_string()]),
                    }]
                }),
            };
            all_tools.push(schema_tool);

            // Provider-specific OAuth device-code helpers
            match connector_name.as_str() {
                "microsoft-graph" => {
                    // start_device
                    let start = Tool {
                        name: format!("auth/{}/start_device", connector_name).into(),
                        title: None,
                        description: Some("Start Microsoft device authorization (returns user_code and verify URL).".into()),
                        input_schema: Arc::new(serde_json::json!({
                            "type":"object",
                            "properties":{
                                "tenant_id": {"type":"string"},
                                "client_id": {"type":"string"},
                                "scopes": {"type":"string", "description":"space-separated scopes, e.g. Mail.Read Calendars.Read"}
                            },
                            "required":["client_id","scopes"]
                        }).as_object().unwrap().clone()),
                        output_schema: None,
                        annotations: None,
                        icons: connector_icon_url.clone().map(|src| {
                            vec![Icon {
                                src,
                                mime_type: Some("image/svg+xml".to_string()),
                                sizes: Some(vec!["any".to_string()]),
                            }]
                        }),
                    };
                    all_tools.push(start);
                    // poll_device
                    let poll = Tool {
                        name: format!("auth/{}/poll_device", connector_name).into(),
                        title: None,
                        description: Some(
                            "Poll token endpoint for device flow using device_code (Microsoft)."
                                .into(),
                        ),
                        input_schema: Arc::new(
                            serde_json::json!({
                                "type":"object",
                                "properties":{
                                    "tenant_id": {"type":"string"},
                                    "client_id": {"type":"string"},
                                    "device_code": {"type":"string"}
                                },
                                "required":["client_id","device_code"]
                            })
                            .as_object()
                            .unwrap()
                            .clone(),
                        ),
                        output_schema: None,
                        annotations: None,
                        icons: connector_icon_url.clone().map(|src| {
                            vec![Icon {
                                src,
                                mime_type: Some("image/svg+xml".to_string()),
                                sizes: Some(vec!["any".to_string()]),
                            }]
                        }),
                    };
                    all_tools.push(poll);
                }
                "github" => {
                    let start = Tool {
                        name: format!("auth/{}/start_device", connector_name).into(),
                        title: None,
                        description: Some("Start GitHub device flow (returns user_code and verify URL).".into()),
                        input_schema: Arc::new(serde_json::json!({
                            "type":"object",
                            "properties":{
                                "client_id": {"type":"string"},
                                "scope": {"type":"string", "description":"space-separated scopes, e.g. repo read:org"}
                            },
                            "required":["client_id"]
                        }).as_object().unwrap().clone()),
                        output_schema: None,
                        annotations: None,
                        icons: connector_icon_url.clone().map(|src| {
                            vec![Icon {
                                src,
                                mime_type: Some("image/svg+xml".to_string()),
                                sizes: Some(vec!["any".to_string()]),
                            }]
                        }),
                    };
                    all_tools.push(start);
                    let poll = Tool {
                        name: format!("auth/{}/poll_device", connector_name).into(),
                        title: None,
                        description: Some("Poll GitHub for access token using device_code.".into()),
                        input_schema: Arc::new(serde_json::json!({
                            "type":"object",
                            "properties":{
                                "client_id": {"type":"string"},
                                "device_code": {"type":"string"},
                                "client_secret": {"type":"string", "description":"optional for OAuth App"}
                            },
                            "required":["client_id","device_code"]
                        }).as_object().unwrap().clone()),
                        output_schema: None,
                        annotations: None,
                        icons: connector_icon_url.clone().map(|src| {
                            vec![Icon {
                                src,
                                mime_type: Some("image/svg+xml".to_string()),
                                sizes: Some(vec!["any".to_string()]),
                            }]
                        }),
                    };
                    all_tools.push(poll);
                }
                _ => {}
            }
        }

        Ok(ListToolsResult {
            tools: all_tools,
            next_cursor: None,
        })
    }

    /// Handle connectors/list request.
    pub async fn handle_list_connectors(&self) -> Result<ListConnectorsResult, ConnectorError> {
        self.handle_list_connectors_with_params(None).await
    }

    pub async fn handle_list_connectors_with_params(
        &self,
        params: Option<ListConnectorsParams>,
    ) -> Result<ListConnectorsResult, ConnectorError> {
        let registry = self.registry.lock().await;
        let mut connectors = Vec::new();
        let probe_auth = params.map(|p| p.probe_auth).unwrap_or(false);

        fn is_probe_safe(name: &str) -> bool {
            // Avoid triggering OS permission prompts or personal-data access during discovery.
            !matches!(
                name,
                "apple-mail"
                    | "apple-notes"
                    | "apple-messages"
                    | "apple-reminders"
                    | "apple-contacts"
                    | "apple-health"
                    | "google-calendar"
                    | "google-drive"
                    | "google-gmail"
                    | "google-people"
                    | "imap"
                    | "smtp"
                    | "macos"
                    | "microsoft-graph"
                    | "spotlight"
            )
        }

        fn has_credentials(schema: &ConnectorConfigSchema, details: &AuthDetails) -> bool {
            let required_fields: Vec<&crate::capabilities::Field> =
                schema.fields.iter().filter(|f| f.required).collect();
            if !required_fields.is_empty() {
                return required_fields
                    .iter()
                    .all(|f| details.get(&f.name).is_some_and(|v| !v.trim().is_empty()));
            }

            let secret_fields: Vec<&crate::capabilities::Field> = schema
                .fields
                .iter()
                .filter(|f| matches!(f.field_type, FieldType::Secret))
                .collect();
            if !secret_fields.is_empty() {
                return secret_fields
                    .iter()
                    .any(|f| details.get(&f.name).is_some_and(|v| !v.trim().is_empty()));
            }

            details.values().any(|v| !v.trim().is_empty())
        }

        for (name, connector) in registry.providers.iter() {
            let c = connector.lock().await;

            let tools_result = c.list_tools(None).await.unwrap_or(ListToolsResult {
                tools: vec![],
                next_cursor: None,
            });

            let schema = c.config_schema();
            let auth_details = match c.get_auth_details().await {
                Ok(details) => details,
                Err(e) => {
                    error!("Error reading auth details for connector {}: {:?}", name, e);
                    AuthDetails::new()
                }
            };
            let credentials_present = has_credentials(&schema, &auth_details);

            let tools_require_auth = tools_result
                .tools
                .iter()
                .any(|tool| tool_requires_auth(tool) == Some(true));
            let requires_auth = c.requires_auth() || tools_require_auth;
            let inferred_slots = infer_auth_slots(name.as_str(), &**c, &schema, requires_auth);
            let supports_anonymous_read = !requires_auth
                || tools_result.tools.iter().any(|tool| {
                    tool_requires_auth(tool).is_some_and(|req| !req)
                        && tool_is_user_facing_read(tool)
                });

            let auth_status = if !requires_auth {
                AuthStatus::NotRequired
            } else if !credentials_present {
                AuthStatus::NeedsSetup
            } else if !probe_auth || !is_probe_safe(name.as_str()) {
                AuthStatus::Unknown
            } else {
                match c.test_auth().await {
                    Ok(()) => AuthStatus::Ready,
                    Err(ConnectorError::Authentication(_)) => AuthStatus::Invalid,
                    Err(ConnectorError::InvalidInput(_)) => AuthStatus::NeedsSetup,
                    Err(ConnectorError::InvalidParams(_)) => AuthStatus::NeedsSetup,
                    Err(_) => AuthStatus::Unknown,
                }
            };

            connectors.push(ConnectorMetadata {
                connector_id: name.clone(),
                name: name.clone(),
                display_name: c.display_name().to_string(),
                description: c.description().to_string(),
                icon: c.icon().to_string(),
                icon_url: icon_url_from_slug(c.icon()),
                tools_count: tools_result.tools.len(),
                tools: tools_result
                    .tools
                    .iter()
                    .map(|t| t.name.to_string())
                    .collect(),
                categories: c.categories().iter().map(|s| s.to_string()).collect(),
                url_patterns: c.url_patterns(),
                supports_anonymous_read,
                requires_auth,
                auth_slots: inferred_slots,
                auth_required: requires_auth,
                auth_status,
            });
        }

        connectors.sort_by(|a, b| a.display_name.cmp(&b.display_name));

        Ok(ListConnectorsResult { connectors })
    }

    /// Handle connectors/ingest_sources request.
    pub async fn handle_list_ingest_sources(
        &self,
    ) -> Result<ListIngestSourcesResult, ConnectorError> {
        self.handle_list_ingest_sources_with_params(None).await
    }

    pub async fn handle_list_ingest_sources_with_params(
        &self,
        params: Option<ListIngestSourcesParams>,
    ) -> Result<ListIngestSourcesResult, ConnectorError> {
        let registry = self.registry.lock().await;
        let mut ingest_sources = Vec::new();

        let params = params.unwrap_or_default();
        let requested_connectors: std::collections::HashSet<String> = params
            .connectors
            .iter()
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        let requested_categories: std::collections::HashSet<String> = params
            .categories
            .iter()
            .map(|c| c.trim().to_lowercase())
            .filter(|c| !c.is_empty())
            .collect();
        let filter_connectors = !requested_connectors.is_empty();
        let filter_categories = !requested_categories.is_empty();

        fn is_ingest_excluded_category(category: &str) -> bool {
            matches!(category, "resolve" | "download" | "export")
        }

        fn should_include_by_default(
            category: &str,
            include_read: bool,
            include_fetch: bool,
            supports_cursor: bool,
        ) -> bool {
            match category {
                "list" | "search" => true,
                "read" => {
                    if supports_cursor {
                        include_read
                    } else {
                        include_fetch
                    }
                }
                _ => false,
            }
        }

        fn suggested_limit_for_category(category: &str) -> i64 {
            match category {
                "read" => 50,
                _ => 25,
            }
        }

        fn clamp_limit_to_schema(prop: &Value, suggested: i64) -> i64 {
            let mut value = suggested;

            let maximum = prop.get("maximum").and_then(|v| v.as_f64());
            if let Some(max) = maximum {
                value = value.min(max.floor() as i64);
            }

            let minimum = prop.get("minimum").and_then(|v| v.as_f64());
            if let Some(min) = minimum {
                value = value.max(min.ceil() as i64);
            }

            value.max(1)
        }

        for (connector_name, connector) in registry.providers.iter() {
            if filter_connectors && !requested_connectors.contains(connector_name) {
                continue;
            }

            let c = connector.lock().await;
            let connector_display = c.display_name().to_string();
            let connector_auth_required = c.requires_auth();
            let tools_result = c.list_tools(None).await.unwrap_or(ListToolsResult {
                tools: vec![],
                next_cursor: None,
            });

            let tools_require_auth = tools_result
                .tools
                .iter()
                .any(|tool| tool_requires_auth(tool) == Some(true));
            let connector_requires_auth = connector_auth_required || tools_require_auth;
            let connector_schema = c.config_schema();
            let inferred_slots = infer_auth_slots(
                connector_name.as_str(),
                &**c,
                &connector_schema,
                connector_requires_auth,
            );

            for tool in tools_result.tools {
                let meta = tool.input_schema.get("_meta").and_then(|v| v.as_object());
                let supports_output = meta
                    .and_then(|m| m.get("supports_output_format"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !supports_output {
                    continue;
                }

                let supports_cursor = meta
                    .and_then(|m| m.get("supports_cursor"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let category = meta
                    .and_then(|m| m.get("category"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_lowercase());

                if let Some(ref cat) = category {
                    if is_ingest_excluded_category(cat.as_str()) {
                        continue;
                    }

                    if filter_categories && !requested_categories.contains(cat) {
                        continue;
                    }

                    if !should_include_by_default(
                        cat.as_str(),
                        params.include_read,
                        params.include_fetch,
                        supports_cursor,
                    ) {
                        continue;
                    }
                } else {
                    // If no category is declared, exclude by default; callers can still use `tools/list`.
                    continue;
                }

                let tags = meta
                    .and_then(|m| m.get("tags"))
                    .and_then(|v| v.as_array())
                    .map(|vals| {
                        vals.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<String>>()
                    })
                    .unwrap_or_default();

                let mut default_args = JsonMap::new();
                default_args.insert("output_format".to_string(), json!("normalized_v1"));
                if let Some(props) = tool
                    .input_schema
                    .get("properties")
                    .and_then(|v| v.as_object())
                {
                    if let Some(limit_prop) = props.get("limit") {
                        if let Some(default_val) = limit_prop.get("default") {
                            let mut numeric_default = default_val.as_i64();
                            if numeric_default.is_none() {
                                numeric_default = default_val.as_f64().map(|v| v.floor() as i64);
                            }
                            if let Some(value) = numeric_default {
                                let clamped = clamp_limit_to_schema(limit_prop, value);
                                default_args.insert("limit".to_string(), json!(clamped));
                            } else {
                                default_args.insert("limit".to_string(), default_val.clone());
                            }
                        } else if let Some(cat) = category.as_deref() {
                            let suggested = suggested_limit_for_category(cat);
                            let clamped = clamp_limit_to_schema(limit_prop, suggested);
                            default_args.insert("limit".to_string(), json!(clamped));
                        }
                    }
                }

                let tool_name = format!("{}/{}", connector_name, tool.name);
                let display_name = tool
                    .title
                    .as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| format!("{} {}", connector_display, tool.name));

                let mut input_schema = tool.input_schema.as_ref().clone();
                inject_tool_auth_meta(&mut input_schema, &inferred_slots);

                ingest_sources.push(IngestSource {
                    id: format!("{}:{}", connector_name, tool.name),
                    connector: connector_name.clone(),
                    display_name,
                    description: tool.description.as_ref().map(|d| d.to_string()),
                    tool: tool_name,
                    input_schema,
                    default_args,
                    category,
                    tags,
                    auth_required: connector_auth_required,
                });
            }
        }

        ingest_sources.sort_by(|a, b| a.display_name.cmp(&b.display_name));

        Ok(ListIngestSourcesResult { ingest_sources })
    }

    /// Handle call_tool request - routes to appropriate connector
    pub async fn handle_call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let requested_display_v1 = request
            .arguments
            .as_ref()
            .and_then(|args| args.get("output_format"))
            .and_then(|v| v.as_str())
            .is_some_and(|v| v == "display_v1");

        // Support auth tools: auth/<provider>/set|test|get_schema
        if request.name.starts_with("auth/") {
            let parts: Vec<&str> = request.name.split('/').collect();
            if parts.len() != 3 {
                return Err(ConnectorError::InvalidInput(
                    "Auth tool must be 'auth/<provider>/<action>'".into(),
                ));
            }
            let provider = parts[1];
            let action = parts[2];

            let registry = self.registry.lock().await;
            let connector = registry
                .providers
                .get(provider)
                .ok_or_else(|| {
                    ConnectorError::InvalidInput(format!("Unknown connector: {}", provider))
                })?
                .clone();

            match action {
                "set" => {
                    // Accept arbitrary object matching connector config schema
                    let args_map = request.arguments.unwrap_or_default();
                    let details: AuthDetails = serde_json::from_value(serde_json::Value::Object(
                        args_map,
                    ))
                    .map_err(|error| {
                        ConnectorError::InvalidParams(format!("Invalid auth details: {error}"))
                    })?;
                    let mut c = connector.lock().await;
                    c.set_auth_details(details).await?;
                    return structured_result_with_text(&serde_json::json!({"ok": true}), None);
                }
                "test" => {
                    let c = connector.lock().await;
                    c.test_auth().await?;
                    return structured_result_with_text(&serde_json::json!({"ok": true}), None);
                }
                "get_schema" => {
                    let c = connector.lock().await;
                    let schema = c.config_schema();
                    let js = config_schema_to_jsonschema(&schema);
                    return structured_result_with_text(&serde_json::json!({"schema": js}), None);
                }
                // Device flow helpers: forward to connector tools
                "start_device" => {
                    let mut req = request.clone();
                    req.name = "auth_start".into();
                    let c = connector.lock().await;
                    return c.call_tool(req).await;
                }
                "poll_device" => {
                    let mut req = request.clone();
                    req.name = "auth_poll".into();
                    let c = connector.lock().await;
                    return c.call_tool(req).await;
                }
                _ => return Err(ConnectorError::ToolNotFound),
            }
        }

        // Parse connector name from tool name. Accept either "connector/tool"
        // (internal canonical form) or "connector.tool" (HTTP catalog form).
        // Both reach this path in practice — don't force callers to know which.
        let name_str = request.name.as_ref();
        let parts: Vec<&str> = name_str.splitn(2, ['/', '.']).collect();
        if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
            return Err(self.tool_name_error(name_str).await);
        }

        let connector_name = parts[0];
        let tool_name = parts[1];

        let registry = self.registry.lock().await;
        let (resolved_connector, resolved_tool) = runtime_tool_aliases(&registry)
            .into_iter()
            .find(|alias| alias.alias_connector == connector_name && alias.alias_tool == tool_name)
            .map(|alias| {
                (
                    alias.target_connector.to_string(),
                    alias.target_tool.to_string(),
                )
            })
            .unwrap_or_else(|| (connector_name.to_string(), tool_name.to_string()));

        if let Some(connector) = registry.get_provider(&resolved_connector) {
            // Create a new request with the unprefixed tool name
            let mut unprefixed_request = CallToolRequestParam {
                name: resolved_tool.into(),
                arguments: request.arguments,
            };

            if requested_display_v1 {
                let mut args = unprefixed_request.arguments.unwrap_or_default();
                args.insert("output_format".to_string(), json!("normalized_v1"));
                unprefixed_request.arguments = Some(args);
            }

            let c = connector.lock().await;
            let mut result = c.call_tool(unprefixed_request).await?;
            if requested_display_v1 && !result.is_error.unwrap_or(false) {
                if let Some(structured) = result.structured_content.as_ref() {
                    if let Some(converted) =
                        try_convert_normalized_structured_content_to_display_v1(structured)?
                    {
                        stash_original_structured_content_in_meta(
                            &mut result.meta,
                            structured,
                            "normalized_v1",
                        );
                        result.structured_content = Some(converted);
                    }
                }
            }
            Ok(result)
        } else {
            drop(registry);
            Err(self.tool_name_error(name_str).await)
        }
    }

    /// Build an actionable error when a tool name didn't route. An opaque
    /// "must be in format connector/tool" teaches the agent nothing; this
    /// tells it what it sent and what it could send instead (case-insensitive
    /// connector match → list that connector's tools; otherwise point at
    /// tools/list with a concrete example).
    async fn tool_name_error(&self, requested: &str) -> ConnectorError {
        let registry = self.registry.lock().await;
        let connector_names: Vec<String> = registry
            .providers
            .keys()
            .cloned()
            .chain(registry.aliases.keys().cloned())
            .collect();

        let matched = connector_names
            .iter()
            .find(|c| c.eq_ignore_ascii_case(requested))
            .cloned();

        let matched_provider = matched
            .as_ref()
            .and_then(|n| registry.get_provider(n).cloned().map(|p| (n.clone(), p)));
        drop(registry);

        if let Some((connector_name, provider)) = matched_provider {
            let c = provider.lock().await;
            if let Ok(listed) = c.list_tools(None).await {
                let names: Vec<String> = listed
                    .tools
                    .iter()
                    .map(|t| format!("{}.{}", connector_name, t.name))
                    .collect();
                let example = names.first().cloned().unwrap_or_default();
                return ConnectorError::InvalidInput(format!(
                    "'{requested}' is a connector name, not a tool. Call one of its tools: {list}. Example: name=\"{example}\".",
                    requested = requested,
                    list = names.join(", "),
                    example = example,
                ));
            }
        }

        ConnectorError::InvalidInput(format!(
            "Tool '{requested}' not found. Tool names look like '<connector>.<action>' (e.g. 'youtube.get', 'hackernews.search'); call the 'tools/list' method to see the full set. Got: '{requested}'.",
            requested = requested,
        ))
    }

    /// Handle list_prompts request - aggregates from all connectors
    pub async fn handle_list_prompts(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        let registry = self.registry.lock().await;
        let mut all_prompts = Vec::new();

        // Collect prompts from all connectors
        for (connector_name, connector) in registry.providers.iter() {
            let c = connector.lock().await;
            match c.list_prompts(request.clone()).await {
                Ok(response) => {
                    // Prefix prompt names with connector name
                    let prefixed_prompts: Vec<Prompt> = response
                        .prompts
                        .into_iter()
                        .map(|mut prompt| {
                            prompt.name = format!("{}/{}", connector_name, prompt.name);
                            prompt
                        })
                        .collect();
                    all_prompts.extend(prefixed_prompts);
                }
                Err(e) => {
                    error!(
                        "Error listing prompts from connector {}: {:?}",
                        connector_name, e
                    );
                }
            }
        }

        Ok(ListPromptsResult {
            prompts: all_prompts,
            next_cursor: None,
        })
    }

    /// Handle get_prompt request - routes to appropriate connector
    pub async fn handle_get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        // Parse connector name from prompt name
        let parts: Vec<&str> = name.split('/').collect();
        if parts.len() != 2 {
            return Err(ConnectorError::InvalidInput(format!(
                "Prompt name must be in format 'connector/prompt', got: {}",
                name
            )));
        }

        let connector_name = parts[0];
        let prompt_name = parts[1];

        let registry = self.registry.lock().await;

        if let Some(connector) = registry.providers.get(connector_name) {
            let c = connector.lock().await;
            let mut prompt = c.get_prompt(prompt_name).await?;
            // Re-prefix the name in the response
            prompt.name = name.to_string();
            Ok(prompt)
        } else {
            Err(ConnectorError::InvalidInput(format!(
                "Unknown connector: {}",
                connector_name
            )))
        }
    }
}

fn config_schema_to_jsonschema(
    schema: &ConnectorConfigSchema,
) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::json;
    let mut props = serde_json::Map::new();
    let mut required: Vec<String> = Vec::new();
    for f in &schema.fields {
        let (ty, extra) = match &f.field_type {
            FieldType::Text => ("string", json!({})),
            FieldType::Secret => ("string", json!({"format":"password","secret":true})),
            FieldType::Number => ("number", json!({})),
            FieldType::Boolean => ("boolean", json!({})),
            FieldType::Select { options } => {
                let opts = options.clone();
                ("string", json!({"enum": opts}))
            }
        };
        let mut obj = serde_json::Map::new();
        obj.insert("type".to_string(), json!(ty));
        obj.insert("title".to_string(), json!(f.label));
        if let Some(desc) = &f.description {
            obj.insert("description".to_string(), json!(desc));
        }
        for (k, v) in extra
            .as_object()
            .expect("Schema extra properties must be an object")
            .iter()
        {
            obj.insert(k.clone(), v.clone());
        }
        props.insert(f.name.clone(), serde_json::Value::Object(obj));
        if f.required {
            required.push(f.name.clone());
        }
    }
    let mut root = serde_json::Map::new();
    root.insert("type".to_string(), json!("object"));
    root.insert("properties".to_string(), serde_json::Value::Object(props));
    if !required.is_empty() {
        root.insert("required".to_string(), json!(required));
    }
    root
}

static CONNECTOR_ICON_DATA_URLS: Lazy<HashMap<String, String>> =
    Lazy::new(load_connector_icon_data_urls);

fn icon_url_from_slug(slug: &str) -> Option<String> {
    let slug = slug.trim();
    if slug.is_empty() || slug == "tool" {
        return None;
    }
    CONNECTOR_ICON_DATA_URLS.get(slug).cloned()
}

fn load_connector_icon_data_urls() -> HashMap<String, String> {
    let mut icon_urls = HashMap::new();

    for dir in connector_icon_dirs() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("svg") {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };

            if icon_urls.contains_key(stem) {
                continue;
            }

            let Ok(bytes) = fs::read(&path) else {
                continue;
            };

            let data_url = format!(
                "data:image/svg+xml;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            );
            icon_urls.insert(stem.to_string(), data_url);
        }
    }

    icon_urls
}

fn connector_icon_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut seen = BTreeSet::new();

    if let Some(asset_root) = crate::paths::resolve_asset_root() {
        push_connector_icon_dir(
            &mut dirs,
            &mut seen,
            asset_root
                .join("resources")
                .join("icons")
                .join("connectors"),
        );
    }

    push_connector_icon_dir(
        &mut dirs,
        &mut seen,
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("resources")
            .join("icons")
            .join("connectors"),
    );

    dirs
}

fn push_connector_icon_dir(dirs: &mut Vec<PathBuf>, seen: &mut BTreeSet<PathBuf>, dir: PathBuf) {
    if dir.is_dir() && seen.insert(dir.clone()) {
        dirs.push(dir);
    }
}

fn tool_requires_auth(tool: &Tool) -> Option<bool> {
    tool.input_schema
        .get("_meta")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get("auth_required"))
        .and_then(|v| v.as_bool())
}

fn tool_is_user_facing_read(tool: &Tool) -> bool {
    let Some(category) = tool
        .input_schema
        .get("_meta")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get("category"))
        .and_then(|v| v.as_str())
    else {
        return false;
    };
    matches!(category, "list" | "search" | "read")
}

fn infer_auth_slots(
    connector_name: &str,
    connector: &dyn crate::Connector,
    schema: &ConnectorConfigSchema,
    requires_auth: bool,
) -> Vec<AuthSlotMetadata> {
    let has_schema_fields = !schema.fields.is_empty();
    // Connectors with optional credentials (e.g. reddit) still advertise
    // their auth method so hosts can offer setup; only connectors with no
    // credential schema at all and no auth requirement get zero slots.
    if !requires_auth && !has_schema_fields {
        return Vec::new();
    }

    let provider_id = if connector_name.starts_with("google-") {
        "google".to_string()
    } else if connector_name == "microsoft-graph" {
        "microsoft".to_string()
    } else {
        connector.credential_provider().to_string()
    };

    let schema_field_names: Vec<String> = schema
        .fields
        .iter()
        .map(|f| f.name.trim().to_lowercase())
        .collect();

    let (slot_id, auth_method_id, auth_kind) = if connector_name == "imap" {
        (
            "mailbox".to_string(),
            "mailbox".to_string(),
            AuthKind::Config,
        )
    } else if connector_name == "smtp" {
        ("outbox".to_string(), "smtp".to_string(), AuthKind::Config)
    } else if connector_name.starts_with("google-")
        || connector_name == "microsoft-graph"
        || schema_field_names.iter().any(|n| {
            matches!(
                n.as_str(),
                "access_token" | "refresh_token" | "client_id" | "client_secret" | "tenant_id"
            )
        })
    {
        (
            "account".to_string(),
            "sign_in".to_string(),
            AuthKind::Oauth2,
        )
    } else if schema.fields.iter().any(|f| {
        if !matches!(f.field_type, FieldType::Secret) {
            return false;
        }
        let name = f.name.trim().to_lowercase();
        name.contains("api_key") || name.contains("token") || name.contains("key")
    }) || schema_field_names
        .iter()
        .any(|n| n.contains("api_key") || n.contains("token") || n.contains("key"))
    {
        (
            "api_key".to_string(),
            "api_key".to_string(),
            AuthKind::ApiKey,
        )
    } else {
        ("config".to_string(), "config".to_string(), AuthKind::Config)
    };

    // Attach the credential field schema for every auth kind so hosts can
    // render setup forms without an extra `auth/<provider>/get_schema` call.
    let config_schema = if has_schema_fields {
        Some(config_schema_to_jsonschema(schema))
    } else {
        None
    };

    vec![AuthSlotMetadata {
        slot_id,
        provider_id,
        auth_method_id,
        auth_kind,
        scopes: Vec::new(),
        config_schema,
        required: requires_auth,
    }]
}

/// Inject `_meta.auth` into a tool's input schema from the connector's first
/// inferred auth slot. Shared by every tools/list aggregation surface
/// (JSON-RPC `tools/list`, `connectors/ingest_sources`) so all of them
/// advertise the same auth contract. Creates `_meta` if absent, preserves
/// existing keys (category/tags/auth_required/...), and never overwrites a
/// pre-existing `_meta.auth`.
pub fn inject_tool_auth_meta(schema_obj: &mut JsonMap<String, Value>, slots: &[AuthSlotMetadata]) {
    let Some(slot) = slots.first() else {
        return;
    };

    let meta_value = schema_obj
        .entry("_meta".to_string())
        .or_insert_with(|| Value::Object(JsonMap::new()));
    let Some(meta) = meta_value.as_object_mut() else {
        return;
    };
    if meta.contains_key("auth") {
        return;
    }

    let required = meta
        .get("auth_required")
        .and_then(|v| v.as_bool())
        .unwrap_or(slot.required);
    // AuthKind serializes as snake_case ("api_key" | "oauth2" | "config").
    let kind = serde_json::to_value(&slot.auth_kind).unwrap_or(Value::Null);

    meta.insert(
        "auth".to_string(),
        json!({
            "kind": kind,
            "provider_id": slot.provider_id,
            "method_id": slot.auth_method_id,
            "required": required,
        }),
    );
}

/// Compute the inferred auth slots for a connector, for hosts that aggregate
/// tools without going through the `McpServer` request handlers (e.g. a server
/// embedding the registry directly).
pub fn connector_auth_slots(
    connector_name: &str,
    connector: &dyn crate::Connector,
) -> Vec<AuthSlotMetadata> {
    let schema = connector.config_schema();
    infer_auth_slots(
        connector_name,
        connector,
        &schema,
        connector.requires_auth(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{ContentItem, NormalizedPageV1, OutputFormat, Partial, Source};
    use async_trait::async_trait;
    use rmcp::model::{
        CallToolRequestParam, InitializeRequestParam, InitializeResult, ListPromptsResult,
        ListResourcesResult, ListToolsResult, PaginatedRequestParam, Prompt, ProtocolVersion,
        ReadResourceRequestParam, ResourceContents, ServerCapabilities, Tool,
    };
    use serde_json::json;
    use std::borrow::Cow;
    use std::sync::Arc;

    struct FakeConnector;
    struct StubConnector {
        name: &'static str,
        tools: Vec<&'static str>,
    }

    #[async_trait]
    impl crate::Connector for FakeConnector {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn description(&self) -> &'static str {
            "fake"
        }

        async fn capabilities(&self) -> ServerCapabilities {
            ServerCapabilities::default()
        }

        async fn initialize(
            &self,
            _request: InitializeRequestParam,
        ) -> Result<InitializeResult, ConnectorError> {
            Ok(InitializeResult {
                protocol_version: ProtocolVersion::LATEST,
                capabilities: ServerCapabilities::default(),
                server_info: crate::Implementation {
                    name: "fake".to_string(),
                    title: None,
                    version: "0.0.0".to_string(),
                    icons: None,
                    website_url: None,
                },
                instructions: None,
            })
        }

        async fn list_resources(
            &self,
            _request: Option<PaginatedRequestParam>,
        ) -> Result<ListResourcesResult, ConnectorError> {
            Ok(ListResourcesResult {
                resources: Vec::new(),
                next_cursor: None,
            })
        }

        async fn read_resource(
            &self,
            _request: ReadResourceRequestParam,
        ) -> Result<Vec<ResourceContents>, ConnectorError> {
            Ok(Vec::new())
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParam>,
        ) -> Result<ListToolsResult, ConnectorError> {
            Ok(ListToolsResult {
                tools: vec![Tool {
                    name: Cow::Borrowed("search"),
                    title: None,
                    description: None,
                    input_schema: Arc::new(
                        json!({
                            "type":"object",
                            "properties":{
                                "output_format":{
                                    "type":"string",
                                    "enum":["raw","normalized_v1","display_v1"],
                                    "default":"raw"
                                }
                            },
                            "examples":[{"description":"example","input":{"output_format":"normalized_v1"}}],
                            "_meta":{
                                "category":"search",
                                "supports_output_format": true,
                                "supports_cursor": false,
                                "auth_required": false
                            }
                        })
                        .as_object()
                        .expect("schema object")
                        .clone(),
                    ),
                    output_schema: None,
                    annotations: None,
                    icons: None,
                }],
                next_cursor: None,
            })
        }

        async fn call_tool(
            &self,
            request: CallToolRequestParam,
        ) -> Result<CallToolResult, ConnectorError> {
            let args = request.arguments.unwrap_or_default();
            let output_format = crate::ingest::output_format_from_args(&args)?;
            assert_eq!(output_format, OutputFormat::NormalizedV1);

            let item = ContentItem {
                item_ref: "fake:item:1".to_string(),
                kind: "thing".to_string(),
                canonical_url: None,
                title: Some("Hello".to_string()),
                created_at: None,
                source_updated_at: None,
                authors: Vec::new(),
                tags: Vec::new(),
                metadata: None,
                blocks: Vec::new(),
                relationships: Vec::new(),
                truncation: None,
            };
            let page = NormalizedPageV1::new(
                vec![item],
                None,
                false,
                Partial::complete(None),
                Source::new("fake", request.name.as_ref()),
            );
            crate::utils::structured_result(&page)
        }

        async fn list_prompts(
            &self,
            _request: Option<PaginatedRequestParam>,
        ) -> Result<ListPromptsResult, ConnectorError> {
            Ok(ListPromptsResult {
                prompts: Vec::new(),
                next_cursor: None,
            })
        }

        async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
            Err(ConnectorError::ToolNotFound)
        }

        async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
            Ok(AuthDetails::new())
        }

        async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
            Ok(())
        }

        async fn test_auth(&self) -> Result<(), ConnectorError> {
            Ok(())
        }

        fn config_schema(&self) -> ConnectorConfigSchema {
            ConnectorConfigSchema::default()
        }
    }

    #[async_trait]
    impl crate::Connector for StubConnector {
        fn name(&self) -> &'static str {
            self.name
        }

        fn description(&self) -> &'static str {
            self.name
        }

        async fn capabilities(&self) -> ServerCapabilities {
            ServerCapabilities::default()
        }

        async fn initialize(
            &self,
            _request: InitializeRequestParam,
        ) -> Result<InitializeResult, ConnectorError> {
            Ok(InitializeResult {
                protocol_version: ProtocolVersion::LATEST,
                capabilities: ServerCapabilities::default(),
                server_info: crate::Implementation {
                    name: self.name.to_string(),
                    title: None,
                    version: "0.0.0".to_string(),
                    icons: None,
                    website_url: None,
                },
                instructions: None,
            })
        }

        async fn list_resources(
            &self,
            _request: Option<PaginatedRequestParam>,
        ) -> Result<ListResourcesResult, ConnectorError> {
            Ok(ListResourcesResult {
                resources: Vec::new(),
                next_cursor: None,
            })
        }

        async fn read_resource(
            &self,
            _request: ReadResourceRequestParam,
        ) -> Result<Vec<ResourceContents>, ConnectorError> {
            Ok(Vec::new())
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParam>,
        ) -> Result<ListToolsResult, ConnectorError> {
            let tools = self
                .tools
                .iter()
                .map(|tool_name| Tool {
                    name: Cow::Owned((*tool_name).to_string()),
                    title: None,
                    description: Some(Cow::Owned(format!("{} {}", self.name, tool_name))),
                    input_schema: Arc::new(
                        json!({
                            "type":"object",
                            "properties":{},
                            "_meta":{
                                "category":"read",
                                "supports_output_format": false,
                                "supports_cursor": false,
                                "auth_required": false
                            }
                        })
                        .as_object()
                        .expect("schema object")
                        .clone(),
                    ),
                    output_schema: None,
                    annotations: None,
                    icons: None,
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
            crate::utils::structured_result_with_text(
                &json!({
                    "connector": self.name,
                    "tool": request.name.as_ref(),
                }),
                None,
            )
        }

        async fn list_prompts(
            &self,
            _request: Option<PaginatedRequestParam>,
        ) -> Result<ListPromptsResult, ConnectorError> {
            Ok(ListPromptsResult {
                prompts: Vec::new(),
                next_cursor: None,
            })
        }

        async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
            Err(ConnectorError::ToolNotFound)
        }

        async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
            Ok(AuthDetails::new())
        }

        async fn set_auth_details(&mut self, _details: AuthDetails) -> Result<(), ConnectorError> {
            Ok(())
        }

        async fn test_auth(&self) -> Result<(), ConnectorError> {
            Ok(())
        }

        fn config_schema(&self) -> ConnectorConfigSchema {
            ConnectorConfigSchema::default()
        }
    }

    #[tokio::test]
    async fn mcp_call_tool_display_v1_stashes_original_normalized() {
        let mut registry = ProviderRegistry::new();
        registry.register_provider(Box::new(FakeConnector));
        let server = McpServer::new(Arc::new(Mutex::new(registry)));

        let result = server
            .handle_call_tool(CallToolRequestParam {
                name: "fake/search".to_string().into(),
                arguments: Some(
                    json!({"output_format":"display_v1"})
                        .as_object()
                        .expect("object")
                        .clone(),
                ),
            })
            .await
            .expect("call");

        let structured = result.structured_content.expect("structured");
        assert_eq!(
            structured.get("type").and_then(|v| v.as_str()),
            Some(crate::display::v1::DISPLAY_PAGE_V1_TYPE)
        );

        let meta = result.meta.expect("meta");
        let original = meta
            .0
            .get(crate::display::from_normalized::META_ORIGINAL_STRUCTURED_CONTENT_KEY)
            .expect("original structured");
        assert_eq!(
            original.get("type").and_then(|v| v.as_str()),
            Some(crate::ingest::NORMALIZED_PAGE_V1_TYPE)
        );
    }

    #[test]
    fn tools_call_error_hook_emits_sanitized_flow_failure_draft() {
        let error = ConnectorError::Other(
            "Slack response channel C123 user U456 text 'private' trace_id abc123".to_string(),
        );
        let request = CallToolRequestParam {
            name: "slack/post-message".to_string().into(),
            arguments: Some(
                json!({
                    "channel": "C123",
                    "text": "private"
                })
                .as_object()
                .expect("object")
                .clone(),
            ),
        };

        let jsonrpc_error = super::jsonrpc_error_with_flow_failure_draft(&request, &error);
        let draft = jsonrpc_error
            .pointer("/data/flow_failure_report_draft")
            .expect("draft");

        assert_eq!(
            draft.get("source").and_then(|value| value.as_str()),
            Some("rzn-tools")
        );
        assert_eq!(
            draft.get("surface").and_then(|value| value.as_str()),
            Some("slack")
        );
        assert_eq!(
            draft.get("flow").and_then(|value| value.as_str()),
            Some("slack/post-message-v1")
        );

        let serialized = serde_json::to_string(draft).unwrap();
        assert!(!serialized.contains("C123"));
        assert!(!serialized.contains("U456"));
        assert!(!serialized.contains("private"));
        assert!(!serialized.contains("trace_id"));
        assert!(draft.get("arguments").is_none());
        assert!(draft.get("response").is_none());
    }

    #[tokio::test]
    async fn runtime_system_aliases_are_routed_but_not_listed() {
        // Per Anthropic's "Writing tools for agents" guidance, aliases must
        // remain callable for backwards compat but must NOT appear in the
        // tool listing — duplicate surface wastes agent context and makes
        // tool selection non-deterministic.
        let mut registry = ProviderRegistry::new();
        registry.register_provider(Box::new(StubConnector {
            name: "youtube",
            tools: vec!["search", "get", "list", "resolve_channel"],
        }));
        registry.register_alias("youtube_transcripts", "youtube");
        registry.register_provider(Box::new(StubConnector {
            name: "federated",
            tools: vec!["federated_search"],
        }));
        registry.register_provider(Box::new(StubConnector {
            name: "web",
            tools: vec!["get"],
        }));
        let server = McpServer::new(Arc::new(Mutex::new(registry)));

        let listed_tools = server.handle_list_tools(None).await.expect("tool list");
        let tool_names: Vec<String> = listed_tools
            .tools
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();

        // Canonical names ARE listed.
        assert!(tool_names.contains(&"youtube/search".to_string()));
        assert!(tool_names.contains(&"youtube/get".to_string()));
        assert!(tool_names.contains(&"federated/federated_search".to_string()));
        assert!(tool_names.contains(&"web/get".to_string()));

        // Alias names are NOT listed.
        assert!(!tool_names.contains(&"youtube_transcripts/search".to_string()));
        assert!(!tool_names.contains(&"youtube_transcripts/get".to_string()));
        assert!(!tool_names.contains(&"youtube_transcripts/list".to_string()));
        assert!(!tool_names.contains(&"youtube_transcripts/resolve_channel".to_string()));
        assert!(!tool_names.contains(&"web_search/search".to_string()));
        assert!(!tool_names.contains(&"web_search/get".to_string()));

        let youtube_result = server
            .handle_call_tool(CallToolRequestParam {
                name: "youtube_transcripts/get".to_string().into(),
                arguments: None,
            })
            .await
            .expect("youtube alias call");
        let youtube_payload = youtube_result
            .structured_content
            .expect("youtube alias payload");
        assert_eq!(
            youtube_payload
                .get("connector")
                .and_then(|value| value.as_str()),
            Some("youtube")
        );
        assert_eq!(
            youtube_payload.get("tool").and_then(|value| value.as_str()),
            Some("get")
        );

        let web_search_result = server
            .handle_call_tool(CallToolRequestParam {
                name: "web_search/search".to_string().into(),
                arguments: None,
            })
            .await
            .expect("web search alias call");
        let web_search_payload = web_search_result
            .structured_content
            .expect("web search alias payload");
        assert_eq!(
            web_search_payload
                .get("connector")
                .and_then(|value| value.as_str()),
            Some("federated")
        );
        assert_eq!(
            web_search_payload
                .get("tool")
                .and_then(|value| value.as_str()),
            Some("federated_search")
        );

        let web_get_result = server
            .handle_call_tool(CallToolRequestParam {
                name: "web_search/get".to_string().into(),
                arguments: None,
            })
            .await
            .expect("web get alias call");
        let web_get_payload = web_get_result
            .structured_content
            .expect("web get alias payload");
        assert_eq!(
            web_get_payload
                .get("connector")
                .and_then(|value| value.as_str()),
            Some("web")
        );
        assert_eq!(
            web_get_payload.get("tool").and_then(|value| value.as_str()),
            Some("get")
        );
    }

    #[cfg(feature = "hackernews")]
    #[tokio::test]
    async fn prefixed_hackernews_tools_expose_string_friendly_id_schema() {
        let mut registry = ProviderRegistry::new();
        registry.register_provider(Box::new(
            crate::connectors::hackernews::HackerNewsConnector::new(),
        ));
        let server = McpServer::new(Arc::new(Mutex::new(registry)));

        let listed_tools = server.handle_list_tools(None).await.expect("tool list");

        for tool_name in ["hackernews/get_thread", "hackernews/get"] {
            let tool = listed_tools
                .tools
                .iter()
                .find(|tool| tool.name.as_ref() == tool_name)
                .unwrap_or_else(|| panic!("missing tool {tool_name}"));

            let props = tool
                .input_schema
                .get("properties")
                .and_then(|value| value.as_object())
                .expect("schema properties");

            let id_types = props
                .get("id")
                .and_then(|value| value.get("type"))
                .expect("id type");
            let item_id_types = props
                .get("item_id")
                .and_then(|value| value.get("type"))
                .expect("item_id type");

            let has_string_type = |value: &Value| {
                value
                    .as_array()
                    .is_some_and(|types| types.iter().any(|entry| entry.as_str() == Some("string")))
            };

            assert!(
                has_string_type(id_types),
                "{tool_name} id schema should accept strings"
            );
            assert!(
                has_string_type(item_id_types),
                "{tool_name} item_id schema should accept strings"
            );
        }
    }
}

/// JSON-RPC message handler for the MCP server
pub struct JsonRpcHandler {
    server: McpServer,
}

fn jsonrpc_error_with_flow_failure_draft(
    request: &CallToolRequestParam,
    error: &ConnectorError,
) -> Value {
    let mut jsonrpc_error = error.to_jsonrpc_error();
    let (surface, tool) = split_tool_name_for_failure_report(request.name.as_ref());
    let draft = build_tool_flow_failure_draft(
        surface,
        tool,
        env!("CARGO_PKG_VERSION"),
        &error.to_string(),
        env!("CARGO_PKG_VERSION"),
        None,
    );

    if let Some(object) = jsonrpc_error.as_object_mut() {
        object.insert(
            "data".to_string(),
            json!({
                "flow_failure_report_draft": draft
            }),
        );
    }

    jsonrpc_error
}

fn split_tool_name_for_failure_report(name: &str) -> (&str, &str) {
    name.split_once('/')
        .or_else(|| name.split_once('.'))
        .unwrap_or(("unknown", name))
}

impl JsonRpcHandler {
    pub fn new(server: McpServer) -> Self {
        Self { server }
    }

    /// Process a JSON-RPC request and return a response
    pub async fn handle_request(&self, request: Value) -> Value {
        debug!("Handling JSON-RPC request: {:?}", request);

        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let result = match method {
            "authorization/describe" => {
                // Static scheme map based on current connectors
                let schemes = json!({
                    "schemes": [
                        {
                            "provider": "reddit",
                            "type": "basic",
                            "fields": [
                                {"name": "client_id", "label": "Client ID", "kind": "string", "required": true},
                                {"name": "client_secret", "label": "Client Secret", "kind": "secret", "required": true},
                                {"name": "username", "label": "Username", "kind": "string", "required": true},
                                {"name": "password", "label": "Password", "kind": "secret", "required": true}
                            ],
                            "notes": "Uses Reddit 'script' OAuth internally; public endpoints still work anonymously.",
                            "requires_auth": "optional"
                        },
                        {
                            "provider": "x",
                            "type": "basic",
                            "fields": [
                                {"name": "username", "label": "Username", "kind": "string", "required": true},
                                {"name": "password", "label": "Password", "kind": "secret", "required": true}
                            ],
                            "hints": {"browser_cookies": true},
                            "notes": "Login or import browser cookies for higher reliability.",
                            "requires_auth": "optional"
                        },
                        {
                            "provider": "semantic-scholar",
                            "type": "api_key",
                            "fields": [
                                {"name": "SEMANTIC_SCHOLAR_API_KEY", "label": "API Key", "kind": "secret", "required": true}
                            ],
                            "requires_auth": "optional"
                        },
                        {"provider": "youtube", "type": "none", "hints": {"browser_cookies": true}},
                        {"provider": "web", "type": "none", "hints": {"browser_cookies": true}},
                        {"provider": "arxiv", "type": "none"},
                        {"provider": "pubmed", "type": "none"},
                        {"provider": "wikipedia", "type": "none"},
                        {"provider": "hackernews", "type": "none"},
                        {"provider": "scihub", "type": "none"}
                    ]
                });
                Ok(schemes)
            }
            "authorization/status" => {
                let map = self.server.auth_status.lock().await.clone();
                let providers: Vec<Value> = map
                    .into_iter()
                    .map(|(provider, st)| {
                        json!({
                            "provider": provider,
                            "authorized": st.authorized,
                            "authorized_at": st.authorized_at,
                        })
                    })
                    .collect();
                Ok(json!({"providers": providers}))
            }
            "secrets/set" => {
                // params: { provider: string, secrets: object }
                let provider = params
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let secrets = params
                    .get("secrets")
                    .and_then(|v| v.as_object())
                    .cloned()
                    .unwrap_or_default();
                if provider.is_empty() {
                    return json!({"error": "Missing provider"});
                } else {
                    // Map secrets (Value map) -> AuthDetails
                    let details = match serde_json::from_value::<AuthDetails>(
                        serde_json::Value::Object(secrets),
                    ) {
                        Ok(details) => details,
                        Err(error) => {
                            return json!(ConnectorError::InvalidParams(format!(
                                "Invalid secrets payload: {error}"
                            ))
                            .to_jsonrpc_error());
                        }
                    };
                    // Apply to connector and test
                    let registry = self.server.registry.lock().await;
                    match registry.providers.get(&provider) {
                        Some(conn) => {
                            let mut c = conn.lock().await;
                            // Map JSON secrets into AuthDetails for the connector
                            if let Err(e) = c.set_auth_details(details).await {
                                return json!(e.to_jsonrpc_error());
                            }
                            if let Err(e) = c.test_auth().await {
                                return json!(e.to_jsonrpc_error());
                            }
                            drop(c);
                            drop(registry);
                            let mut status = self.server.auth_status.lock().await;
                            status.insert(
                                provider.clone(),
                                AuthState {
                                    authorized: true,
                                    authorized_at: Some(chrono::Utc::now().to_rfc3339()),
                                },
                            );
                            Ok(json!({"ok": true}))
                        }
                        None => Err(ConnectorError::InvalidInput(format!(
                            "Unknown provider: {}",
                            provider
                        ))
                        .to_jsonrpc_error()),
                    }
                }
            }
            "secrets/delete" => {
                let provider = params
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if provider.is_empty() {
                    Err(
                        ConnectorError::InvalidParams("Missing provider".to_string())
                            .to_jsonrpc_error(),
                    )
                } else {
                    let mut status = self.server.auth_status.lock().await;
                    status.insert(
                        provider,
                        AuthState {
                            authorized: false,
                            authorized_at: None,
                        },
                    );
                    Ok(json!({"ok": true}))
                }
            }
            "initialize" => match serde_json::from_value::<InitializeRequestParam>(params) {
                Ok(req) => self
                    .server
                    .handle_initialize(req)
                    .await
                    .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
                    .map_err(|e| e.to_jsonrpc_error()),
                Err(e) => Err(ConnectorError::SerdeJson(e).to_jsonrpc_error()),
            },
            "resources/list" => {
                match serde_json::from_value::<Option<PaginatedRequestParam>>(params) {
                    Ok(req) => self
                        .server
                        .handle_list_resources(req)
                        .await
                        .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
                        .map_err(|e| e.to_jsonrpc_error()),
                    Err(e) => Err(ConnectorError::SerdeJson(e).to_jsonrpc_error()),
                }
            }
            "resources/read" => match serde_json::from_value::<ReadResourceRequestParam>(params) {
                Ok(req) => self
                    .server
                    .handle_read_resource(req)
                    .await
                    .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
                    .map_err(|e| e.to_jsonrpc_error()),
                Err(e) => Err(ConnectorError::SerdeJson(e).to_jsonrpc_error()),
            },
            "tools/list" => match serde_json::from_value::<Option<PaginatedRequestParam>>(params) {
                Ok(req) => self
                    .server
                    .handle_list_tools(req)
                    .await
                    .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
                    .map_err(|e| e.to_jsonrpc_error()),
                Err(e) => Err(ConnectorError::SerdeJson(e).to_jsonrpc_error()),
            },
            "connectors/list" => {
                match serde_json::from_value::<Option<ListConnectorsParams>>(params) {
                    Ok(req) => self
                        .server
                        .handle_list_connectors_with_params(req)
                        .await
                        .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
                        .map_err(|e| e.to_jsonrpc_error()),
                    Err(e) => Err(ConnectorError::SerdeJson(e).to_jsonrpc_error()),
                }
            }
            "connectors/ingest_sources" => {
                match serde_json::from_value::<Option<ListIngestSourcesParams>>(params) {
                    Ok(req) => self
                        .server
                        .handle_list_ingest_sources_with_params(req)
                        .await
                        .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
                        .map_err(|e| e.to_jsonrpc_error()),
                    Err(e) => Err(ConnectorError::SerdeJson(e).to_jsonrpc_error()),
                }
            }
            "tools/call" => match serde_json::from_value::<CallToolRequestParam>(params) {
                Ok(req) => match self.server.handle_call_tool(req.clone()).await {
                    Ok(result) => serde_json::to_value(result)
                        .map_err(ConnectorError::SerdeJson)
                        .map_err(|error| error.to_jsonrpc_error()),
                    Err(error) => Err(jsonrpc_error_with_flow_failure_draft(&req, &error)),
                },
                Err(e) => Err(ConnectorError::SerdeJson(e).to_jsonrpc_error()),
            },
            "prompts/list" => {
                match serde_json::from_value::<Option<PaginatedRequestParam>>(params) {
                    Ok(req) => self
                        .server
                        .handle_list_prompts(req)
                        .await
                        .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
                        .map_err(|e| e.to_jsonrpc_error()),
                    Err(e) => Err(ConnectorError::SerdeJson(e).to_jsonrpc_error()),
                }
            }
            "prompts/get" => match params.get("name").and_then(|n| n.as_str()) {
                Some(name) => self
                    .server
                    .handle_get_prompt(name)
                    .await
                    .and_then(|r| serde_json::to_value(r).map_err(ConnectorError::SerdeJson))
                    .map_err(|e| e.to_jsonrpc_error()),
                None => Err(
                    ConnectorError::InvalidInput("Missing 'name' parameter".to_string())
                        .to_jsonrpc_error(),
                ),
            },
            _ => Err(ConnectorError::MethodNotFound.to_jsonrpc_error()),
        };

        match result {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "result": result,
                "id": id,
            }),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "error": error,
                "id": id,
            }),
        }
    }
}
