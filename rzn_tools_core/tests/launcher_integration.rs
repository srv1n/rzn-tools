use std::sync::Arc;

use async_trait::async_trait;
use rmcp::model::*;
use rzn_tools_core::auth::AuthDetails;
use rzn_tools_core::capabilities::ConnectorConfigSchema;
use rzn_tools_core::error::ConnectorError;
use rzn_tools_core::mcp_server::{AuthStatus, ListIngestSourcesParams, McpServer};
use rzn_tools_core::{Connector, ProviderRegistry, URLParamExtraction, URLPatternSpec};
use serde_json::json;
use tokio::sync::Mutex;

struct DummyConnector;

#[async_trait]
impl Connector for DummyConnector {
    fn name(&self) -> &'static str {
        "dummy"
    }

    fn description(&self) -> &'static str {
        "Dummy connector for launcher integration tests."
    }

    fn display_name(&self) -> &'static str {
        "Dummy"
    }

    fn icon(&self) -> &'static str {
        "github"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["test"]
    }

    fn requires_auth(&self) -> bool {
        true
    }

    fn url_patterns(&self) -> Vec<URLPatternSpec> {
        vec![URLPatternSpec {
            pattern: r"example\.com/item/(\d+)".to_string(),
            default_tool: "get".to_string(),
            description: "Fetch a dummy item".to_string(),
            param_extraction: vec![URLParamExtraction {
                capture_group: 1,
                param_name: "id".to_string(),
                use_full_url: false,
            }],
        }]
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
            capabilities: self.capabilities().await,
            server_info: Implementation {
                name: self.name().to_string(),
                title: None,
                version: "0.1.0".to_string(),
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
                name: "list".into(),
                title: Some("List dummy items".into()),
                description: Some("List dummy items (seed tool).".into()),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1"],
                                "default": "raw"
                            }
                        },
                        "required": ["id"],
                        "examples": [
                            { "description": "Example list call", "input": { "id": "seed", "limit": 10 } }
                        ],
                        "_meta": {
                            "category": "list",
                            "tags": ["test"],
                            "auth_required": true,
                            "supports_output_format": true,
                            "supports_cursor": false
                        }
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: "read_window".into(),
                title: Some("Read dummy window".into()),
                description: Some("Read a window of items with cursor paging (schedule tool).".into()),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "cursor": { "type": ["string", "null"], "description": "Opaque cursor." },
                            "limit": { "type": "integer", "minimum": 1, "maximum": 100 },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1"],
                                "default": "raw"
                            }
                        },
                        "required": ["id"],
                        "examples": [
                            { "description": "First page", "input": { "id": "seed", "limit": 50 } },
                            { "description": "Next page", "input": { "id": "seed", "cursor": "opaque", "limit": 50 } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["test"],
                            "auth_required": true,
                            "supports_output_format": true,
                            "supports_cursor": true
                        }
                    })
                    .as_object()
                    .expect("Schema object")
                    .clone(),
                ),
                output_schema: None,
                annotations: None,
                icons: None,
            },
            Tool {
                name: "get".into(),
                title: Some("Get dummy item".into()),
                description: Some("Fetch a single item (fetch tool; not an ingest source by default).".into()),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "output_format": {
                                "type": "string",
                                "enum": ["raw", "normalized_v1"],
                                "default": "raw"
                            }
                        },
                        "required": ["id"],
                        "examples": [
                            { "description": "Example get call", "input": { "id": "123" } }
                        ],
                        "_meta": {
                            "category": "read",
                            "tags": ["test"],
                            "auth_required": true,
                            "supports_output_format": true,
                            "supports_cursor": false
                        }
                    })
                    .as_object()
                    .expect("Schema object")
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
        _request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        Err(ConnectorError::ToolNotFound)
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
        ConnectorConfigSchema { fields: Vec::new() }
    }
}

#[tokio::test]
async fn test_connectors_list_endpoint() {
    let mut registry = ProviderRegistry::new();
    registry.register_provider(Box::new(DummyConnector));

    let server = McpServer::new(Arc::new(Mutex::new(registry)));
    let result = server.handle_list_connectors().await.unwrap();

    assert_eq!(result.connectors.len(), 1);
    let connector = &result.connectors[0];
    assert_eq!(connector.connector_id, "dummy");
    assert_eq!(connector.name, "dummy");
    assert_eq!(connector.display_name, "Dummy");
    assert_eq!(connector.icon, "github");
    assert!(connector
        .icon_url
        .as_deref()
        .is_some_and(|src| src.starts_with("data:image/svg+xml;base64,")));
    assert_eq!(connector.tools_count, 3);
    assert_eq!(connector.supports_anonymous_read, false);
    assert_eq!(connector.requires_auth, true);
    assert_eq!(connector.auth_slots.len(), 1);
    assert_eq!(connector.auth_required, true);
    assert_eq!(connector.auth_status, AuthStatus::NeedsSetup);
    assert!(!connector.url_patterns.is_empty());
}

#[tokio::test]
async fn test_list_tools_injects_bundled_tool_icons() {
    let mut registry = ProviderRegistry::new();
    registry.register_provider(Box::new(DummyConnector));

    let server = McpServer::new(Arc::new(Mutex::new(registry)));
    let result = server.handle_list_tools(None).await.unwrap();
    let list_tool = result
        .tools
        .iter()
        .find(|tool| tool.name.as_ref() == "dummy/list")
        .expect("dummy/list tool");

    let icon = list_tool
        .icons
        .as_ref()
        .and_then(|icons| icons.first())
        .expect("bundled tool icon");

    assert!(icon.src.starts_with("data:image/svg+xml;base64,"));
    assert_eq!(icon.mime_type.as_deref(), Some("image/svg+xml"));
}

#[tokio::test]
async fn test_ingest_sources_endpoint() {
    let mut registry = ProviderRegistry::new();
    registry.register_provider(Box::new(DummyConnector));

    let server = McpServer::new(Arc::new(Mutex::new(registry)));
    let result = server.handle_list_ingest_sources().await.unwrap();

    // Default behavior: include seed tools (list/search) + windowed reads (read + supports_cursor),
    // but exclude one-shot fetch tools by default.
    assert_eq!(result.ingest_sources.len(), 2);
    let tool_names: Vec<String> = result
        .ingest_sources
        .iter()
        .map(|s| s.tool.clone())
        .collect();
    assert!(tool_names.contains(&"dummy/list".to_string()));
    assert!(tool_names.contains(&"dummy/read_window".to_string()));
    assert!(!tool_names.contains(&"dummy/get".to_string()));

    // Default args should always include normalized output format.
    for source in &result.ingest_sources {
        assert_eq!(source.connector, "dummy");
        assert!(source.default_args.get("output_format").is_some());
        // Default limit should be present when the schema defines `limit`.
        assert!(source.default_args.get("limit").is_some());
    }
}

#[tokio::test]
async fn test_ingest_sources_filters() {
    let mut registry = ProviderRegistry::new();
    registry.register_provider(Box::new(DummyConnector));

    let server = McpServer::new(Arc::new(Mutex::new(registry)));

    // Filter by category=list
    let list_only = server
        .handle_list_ingest_sources_with_params(Some(ListIngestSourcesParams {
            categories: vec!["list".to_string()],
            ..Default::default()
        }))
        .await
        .unwrap();
    assert_eq!(list_only.ingest_sources.len(), 1);
    assert_eq!(list_only.ingest_sources[0].tool, "dummy/list");

    // Filter by category=read (windowed reads only by default)
    let reads_only = server
        .handle_list_ingest_sources_with_params(Some(ListIngestSourcesParams {
            categories: vec!["read".to_string()],
            ..Default::default()
        }))
        .await
        .unwrap();
    assert_eq!(reads_only.ingest_sources.len(), 1);
    assert_eq!(reads_only.ingest_sources[0].tool, "dummy/read_window");

    // Allow fetch tools explicitly
    let reads_with_fetch = server
        .handle_list_ingest_sources_with_params(Some(ListIngestSourcesParams {
            categories: vec!["read".to_string()],
            include_fetch: true,
            ..Default::default()
        }))
        .await
        .unwrap();
    assert_eq!(reads_with_fetch.ingest_sources.len(), 2);
    let tools: Vec<String> = reads_with_fetch
        .ingest_sources
        .iter()
        .map(|s| s.tool.clone())
        .collect();
    assert!(tools.contains(&"dummy/read_window".to_string()));
    assert!(tools.contains(&"dummy/get".to_string()));

    // Filter by connector name
    let none = server
        .handle_list_ingest_sources_with_params(Some(ListIngestSourcesParams {
            connectors: vec!["does-not-exist".to_string()],
            ..Default::default()
        }))
        .await
        .unwrap();
    assert!(none.ingest_sources.is_empty());
}

#[cfg(feature = "arxiv")]
#[tokio::test]
async fn test_arxiv_tool_examples_in_schema() {
    let connector = rzn_tools_core::connectors::arxiv::ArxivConnector::new(AuthDetails::new())
        .await
        .unwrap();
    let tools = connector.list_tools(None).await.unwrap();
    let search_tool = tools.tools.iter().find(|t| t.name == "search").unwrap();
    assert!(search_tool.input_schema.contains_key("examples"));
    let examples = search_tool
        .input_schema
        .get("examples")
        .and_then(|v| v.as_array())
        .unwrap();
    assert!(!examples.is_empty());
    let meta = search_tool
        .input_schema
        .get("_meta")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(meta.get("supports_output_format").unwrap(), &json!(true));
    assert_eq!(meta.get("supports_cursor").unwrap(), &json!(true));
}
