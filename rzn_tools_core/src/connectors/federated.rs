//! Federated search connector.
//!
//! This connector exposes federated search as an MCP tool, allowing AI agents
//! to search across multiple data sources with a single tool call.

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::federated::{FederatedSearch, MergeMode, ProfileStore, SearchProfile};
use crate::utils::structured_result_with_text;
use crate::{
    CallToolRequestParam, CallToolResult, Connector, ListPromptsResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParam, ProviderRegistry, Tool,
};
use async_trait::async_trait;
use rmcp::model::*;
use serde_json::json;
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Federated search connector.
///
/// This connector provides a `federated_search` tool that allows searching
/// across multiple connectors simultaneously using profiles or ad-hoc
/// connector lists.
pub struct FederatedConnector {
    registry: Arc<Mutex<Option<Arc<ProviderRegistry>>>>,
    profile_store: ProfileStore,
}

impl FederatedConnector {
    /// Create a new federated connector.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(None)),
            profile_store: ProfileStore::new_default(),
        }
    }

    /// Set the provider registry.
    ///
    /// This must be called before using federated search.
    pub async fn set_registry(&self, registry: Arc<ProviderRegistry>) {
        let mut guard = self.registry.lock().await;
        *guard = Some(registry);
    }
}

impl Default for FederatedConnector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Connector for FederatedConnector {
    fn name(&self) -> &'static str {
        "federated"
    }

    fn description(&self) -> &'static str {
        "Search across multiple data sources simultaneously using profiles or ad-hoc connector lists"
    }

    fn display_name(&self) -> &'static str {
        "Federated Search"
    }

    fn icon(&self) -> &'static str {
        "federated"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["search", "federated"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    async fn capabilities(&self) -> ServerCapabilities {
        ServerCapabilities {
            tools: None,
            ..Default::default()
        }
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
        ConnectorConfigSchema { fields: vec![] }
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
                title: Some("Federated Search".to_string()),
                version: "0.1.0".to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Federated search connector for searching across multiple data sources \
                simultaneously. Use profiles like 'research', 'enterprise', 'social', \
                'code', or 'web', or specify connectors directly."
                    .to_string(),
            ),
        })
    }

    async fn list_resources(
        &self,
        _params: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        Ok(ListResourcesResult {
            resources: vec![],
            next_cursor: None,
        })
    }

    async fn read_resource(
        &self,
        _params: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        Err(ConnectorError::ResourceNotFound)
    }

    async fn list_tools(
        &self,
        _params: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        let builtin_profiles: Vec<String> = ProfileStore::list_builtin_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        Ok(ListToolsResult {
            tools: vec![Tool {
                name: Cow::Borrowed("federated_search"),
                title: Some("Federated Search".to_string()),
                description: Some(Cow::Owned(format!(
                    "Search across multiple data sources simultaneously. \
                    Use built-in profiles ({}) or specify connectors directly. \
                    Results are grouped by source by default.",
                    builtin_profiles.join(", ")
                ))),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search query"
                            },
                            "profile": {
                                "type": "string",
                                "description": "Named profile to use. Built-in profiles: research (academic papers), enterprise (internal docs), social (forums), code (GitHub), web (AI search)",
                                "enum": builtin_profiles
                            },
                            "connectors": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Ad-hoc list of connector names (alternative to profile). Example: [\"pubmed\", \"arxiv\"]"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum results per source",
                                "default": 10
                            },
                            "merge": {
                                "type": "string",
                                "enum": ["grouped", "interleaved"],
                                "description": "How to merge results. 'grouped' organizes by source, 'interleaved' creates single ranked list",
                                "default": "grouped"
                            }
                        },
                        "required": ["query"]
                    })
                    .as_object()
                    .expect("Schema object")
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
        match request.name.as_ref() {
            "federated_search" => {
                let args = request.arguments.unwrap_or_default();

                // Extract query (required)
                let query = args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                    ConnectorError::InvalidParams("Missing 'query' parameter".to_string())
                })?;

                // Extract merge mode
                let merge_mode = match args.get("merge").and_then(|v| v.as_str()) {
                    Some("interleaved") => MergeMode::Interleaved,
                    _ => MergeMode::Grouped,
                };

                // Get registry
                let registry_guard = self.registry.lock().await;
                let registry = registry_guard.as_ref().ok_or_else(|| {
                    ConnectorError::Other("Registry not set. Call set_registry first.".to_string())
                })?;

                let engine = FederatedSearch::new(registry);

                // Execute search based on profile or connectors
                let result = if let Some(profile_name) =
                    args.get("profile").and_then(|v| v.as_str())
                {
                    // Load profile
                    let mut profile = self.profile_store.load(profile_name).ok_or_else(|| {
                        ConnectorError::InvalidParams(format!(
                            "Profile '{}' not found. Available: {}",
                            profile_name,
                            ProfileStore::list_builtin_names().join(", ")
                        ))
                    })?;

                    // Apply limit override if provided
                    if let Some(limit) = args.get("limit").and_then(|v| v.as_u64()) {
                        profile.defaults.limit = limit as u32;
                    }

                    engine
                        .search_with_profile(query, &profile, Some(merge_mode))
                        .await
                } else if let Some(connectors) = args.get("connectors").and_then(|v| v.as_array()) {
                    // Ad-hoc connector list
                    let connector_names: Vec<String> = connectors
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();

                    if connector_names.is_empty() {
                        return Err(ConnectorError::InvalidParams(
                            "Either 'profile' or 'connectors' must be specified".to_string(),
                        ));
                    }

                    engine
                        .search_adhoc(query, &connector_names, merge_mode)
                        .await
                } else {
                    // Default to research profile
                    let profile = SearchProfile::get_builtin("research").ok_or_else(|| {
                        ConnectorError::Other("Default profile not found".to_string())
                    })?;

                    engine
                        .search_with_profile(query, &profile, Some(merge_mode))
                        .await
                };

                // Convert to structured result
                let result_json = serde_json::to_value(&result)
                    .map_err(|e| ConnectorError::Other(e.to_string()))?;

                // Create summary text
                let summary = format_result_summary(&result);

                structured_result_with_text(&result_json, Some(summary))
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }

    async fn list_prompts(
        &self,
        _params: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        Ok(ListPromptsResult {
            prompts: vec![],
            next_cursor: None,
        })
    }

    async fn get_prompt(&self, _name: &str) -> Result<Prompt, ConnectorError> {
        Err(ConnectorError::Other("No prompts available".to_string()))
    }
}

/// Format a summary of federated search results for text output.
fn format_result_summary(result: &crate::federated::FederatedSearchResult) -> String {
    use crate::federated::FederatedResults;

    let mut lines = vec![];

    lines.push(format!("Search: \"{}\"", result.query));
    if let Some(ref profile) = result.profile {
        lines.push(format!("Profile: {}", profile));
    }

    match &result.results {
        FederatedResults::Grouped { sources } => {
            for source in sources {
                lines.push(format!(
                    "\n== {} ({} results) ==",
                    source.source, source.count
                ));
                for (i, r) in source.results.iter().enumerate().take(5) {
                    let url_str = r.url.as_deref().unwrap_or("");
                    lines.push(format!("{}. [{}] {}", i + 1, r.id, r.title));
                    if !url_str.is_empty() {
                        lines.push(format!("   {}", url_str));
                    }
                }
                if source.results.len() > 5 {
                    lines.push(format!("   ... and {} more", source.results.len() - 5));
                }
            }
        }
        FederatedResults::Interleaved { results } => {
            lines.push(format!("\n{} results (interleaved):", results.len()));
            for (i, r) in results.iter().enumerate().take(10) {
                let url_str = r.url.as_deref().unwrap_or("");
                lines.push(format!("{}. [{}] {} ({})", i + 1, r.id, r.title, r.source));
                if !url_str.is_empty() {
                    lines.push(format!("   {}", url_str));
                }
            }
            if results.len() > 10 {
                lines.push(format!("... and {} more", results.len() - 10));
            }
        }
    }

    if result.partial {
        lines.push("\nPartial results. Errors:".to_string());
        for err in &result.errors {
            let timeout_str = if err.is_timeout { " (timeout)" } else { "" };
            lines.push(format!("  - {}: {}{}", err.source, err.error, timeout_str));
        }
    }

    if let Some(duration) = result.duration_ms {
        lines.push(format!("\nCompleted in {}ms", duration));
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connector_name() {
        let connector = FederatedConnector::new();
        assert_eq!(connector.name(), "federated");
    }

    #[tokio::test]
    async fn test_list_tools() {
        let connector = FederatedConnector::new();
        let result = connector.list_tools(None).await.unwrap();
        assert_eq!(result.tools.len(), 1);
        assert_eq!(result.tools[0].name, "federated_search");
    }
}
