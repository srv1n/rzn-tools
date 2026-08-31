pub mod http;

use std::{collections::HashSet, env, sync::Arc};

use tokio::sync::Mutex;
use tracing::error;

use rzn_tools_core::{
    auth_store::enable_invocation_scoped_auth,
    mcp_server::{JsonRpcHandler, McpServer},
    transport::{StdioTransport, DEFAULT_MAX_FRAME_BYTES},
};

pub const SIDECAR_CAPABILITY_VERSION: &str = "rzn-tools-mcp-capabilities-v1";
pub const SIDECAR_PROTOCOL_VERSION: &str = "2025-03-26";

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

/// Run one tenant-bound MCP child with no inherited credential sources.
pub async fn run_sidecar_server(
    exposed_connectors: Option<HashSet<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    isolate_invocation_environment();
    let handler = build_handler_with_connectors(exposed_connectors).await;
    StdioTransport::with_max_frame_bytes(handler, DEFAULT_MAX_FRAME_BYTES)
        .run()
        .await?;
    Ok(())
}

pub fn isolate_invocation_environment() {
    for (name, _) in env::vars_os() {
        let name = name.to_string_lossy();
        if is_sensitive_env_name(&name) {
            env::remove_var(name.as_ref());
        }
    }
    env::remove_var("RZN_PERSIST_TOKENS");
    env::remove_var("RZN_SHOW_ADMIN_TOOLS");
    enable_invocation_scoped_auth();
}

pub fn is_sensitive_env_name(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("WUZAPI_")
        || name.contains("API_KEY")
        || name.contains("TOKEN")
        || name.contains("SECRET")
        || name.contains("PASSWORD")
        || name.contains("COOKIE")
        || name.contains("OAUTH")
        || name.contains("CREDENTIAL")
        || name.contains("PRIVATE_KEY")
        || name.contains("ACCESS_KEY")
        || name.contains("SECRET_KEY")
        || name.contains("KEY_ID")
        || name.contains("DATABASE_URL")
        || name.contains("CLIENT_ID")
        || name.contains("AUTH")
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

    #[test]
    fn credential_env_names_are_not_ambient_sidecar_inputs() {
        assert!(is_sensitive_env_name("OPENAI_API_KEY"));
        assert!(is_sensitive_env_name("WUZAPI_TOKEN"));
        assert!(is_sensitive_env_name("RZN_REDDIT_OAUTH_BASE_URL"));
        assert!(is_sensitive_env_name("AWS_ACCESS_KEY_ID"));
        assert!(!is_sensitive_env_name("RZN_TOOLS_MCP_CONNECTORS"));
        assert!(!is_sensitive_env_name("HTTP_PROXY"));
    }
}
