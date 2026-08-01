pub mod http;

use std::{collections::HashSet, sync::Arc};

use tokio::sync::Mutex;
use tracing::error;

use rzn_tools_core::{
    mcp_server::{JsonRpcHandler, McpServer},
    transport::StdioTransport,
};

pub use http::{HttpConfig, HttpServer};

pub async fn build_handler() -> JsonRpcHandler {
    build_handler_with_connectors(None).await
}

pub async fn build_handler_with_connectors(
    exposed_connectors: Option<HashSet<String>>,
) -> JsonRpcHandler {
    let registry = match rzn_tools_core::UsageManager::new_default() {
        Ok(usage) => rzn_tools_core::build_registry_enabled_only_with_usage(Arc::new(usage)).await,
        Err(err) => {
            error!(
                "Usage manager init failed, continuing without metering: {}",
                err
            );
            rzn_tools_core::build_registry_enabled_only().await
        }
    };
    let mut registry = registry;
    if let Some(exposed_connectors) = exposed_connectors {
        registry.retain_connectors(&exposed_connectors);
    }

    let registry = Arc::new(Mutex::new(registry));
    let server = McpServer::new(registry);
    JsonRpcHandler::new(server)
}

pub async fn run_stdio_server(
    exposed_connectors: Option<HashSet<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let handler = build_handler_with_connectors(exposed_connectors).await;
    let transport = StdioTransport::new(handler);
    transport.run().await?;
    Ok(())
}

pub async fn run_http_server(config: HttpConfig) -> Result<(), Box<dyn std::error::Error>> {
    let handler = build_handler_with_connectors(config.exposed_connectors.clone()).await;
    let server = HttpServer::new(handler, config);
    server.warm_up().await?;
    server.serve().await?;
    Ok(())
}

#[cfg(all(test, feature = "reddit"))]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn connector_allowlist_applies_to_stdio_handler() {
        let exposed_connectors = HashSet::from(["reddit".to_string()]);
        let handler = build_handler_with_connectors(Some(exposed_connectors)).await;

        let response = handler
            .handle_request(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/list",
                "params": {}
            }))
            .await;
        let tool_names = response["result"]["tools"]
            .as_array()
            .expect("tools/list result")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(|name| name.as_str()))
            .collect::<Vec<_>>();

        assert!(!tool_names.is_empty());
        assert!(tool_names.iter().any(|name| name.starts_with("reddit/")));
        assert!(tool_names
            .iter()
            .all(|name| !name.starts_with("hackernews/")));
    }
}
