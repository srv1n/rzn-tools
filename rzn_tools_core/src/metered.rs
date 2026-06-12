use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use rmcp::model::Meta;
use serde_json::{Map, Value};
use tracing::debug;

use crate::usage::UsageManager;
use crate::usage_context::current_context;
use crate::{
    auth::AuthDetails, CallToolRequestParam, CallToolResult, Connector, ConnectorError,
    InitializeRequestParam, InitializeResult, ListPromptsResult, ListResourcesResult,
    ListToolsResult, PaginatedRequestParam, Prompt, ReadResourceRequestParam, ResourceContents,
    ServerCapabilities,
};

pub struct MeteredConnector {
    inner: Box<dyn Connector>,
    usage: Arc<UsageManager>,
}

impl MeteredConnector {
    pub fn new(inner: Box<dyn Connector>, usage: Arc<UsageManager>) -> Self {
        Self { inner, usage }
    }
}

#[async_trait]
impl Connector for MeteredConnector {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn description(&self) -> &'static str {
        self.inner.description()
    }

    fn credential_provider(&self) -> &'static str {
        self.inner.credential_provider()
    }

    async fn capabilities(&self) -> ServerCapabilities {
        self.inner.capabilities().await
    }

    async fn initialize(
        &self,
        request: InitializeRequestParam,
    ) -> Result<InitializeResult, ConnectorError> {
        self.inner.initialize(request).await
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListResourcesResult, ConnectorError> {
        self.inner.list_resources(request).await
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
    ) -> Result<Vec<ResourceContents>, ConnectorError> {
        self.inner.read_resource(request).await
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        self.inner.list_tools(request).await
    }

    async fn call_tool(
        &self,
        mut request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let tool_name = request.name.to_string();
        let mut model = request
            .arguments
            .as_ref()
            .and_then(|args| args.get("model"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let call_meta = extract_call_meta(&mut request.arguments);
        let run_id = call_meta
            .run_id
            .or_else(|| current_context().map(|ctx| ctx.run_id))
            .unwrap_or_else(|| new_id("run"));
        let request_id = call_meta.request_id.unwrap_or_else(|| new_id("req"));
        let key_id = call_meta.key_id.clone();
        let provider = self.credential_provider();

        let start = Instant::now();
        let result = self.inner.call_tool(request).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(mut ok) => {
                if let Some(Value::String(m)) =
                    ok.structured_content.as_ref().and_then(|v| v.get("model"))
                {
                    model = Some(m.clone());
                }
                let (event, meta) = self.usage.estimate_event(
                    self.name(),
                    &tool_name,
                    provider,
                    &run_id,
                    &request_id,
                    key_id.clone(),
                    "ok",
                    duration_ms,
                    ok.structured_content.as_ref(),
                    model.as_deref(),
                );
                if let Err(err) = self.usage.store.record(&event) {
                    debug!("usage record failed: {}", err);
                }
                ok.meta = merge_meta(ok.meta, meta);
                Ok(ok)
            }
            Err(err) => {
                let (event, _meta) = self.usage.estimate_event(
                    self.name(),
                    &tool_name,
                    provider,
                    &run_id,
                    &request_id,
                    key_id,
                    "error",
                    duration_ms,
                    None,
                    model.as_deref(),
                );
                if let Err(store_err) = self.usage.store.record(&event) {
                    debug!("usage record failed: {}", store_err);
                }
                Err(err)
            }
        }
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParam>,
    ) -> Result<ListPromptsResult, ConnectorError> {
        self.inner.list_prompts(request).await
    }

    async fn get_prompt(&self, name: &str) -> Result<Prompt, ConnectorError> {
        self.inner.get_prompt(name).await
    }

    async fn get_auth_details(&self) -> Result<AuthDetails, ConnectorError> {
        self.inner.get_auth_details().await
    }

    async fn set_auth_details(&mut self, details: AuthDetails) -> Result<(), ConnectorError> {
        self.inner.set_auth_details(details).await
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        self.inner.test_auth().await
    }

    fn config_schema(&self) -> crate::capabilities::ConnectorConfigSchema {
        self.inner.config_schema()
    }
}

#[derive(Debug, Default)]
struct CallMeta {
    run_id: Option<String>,
    request_id: Option<String>,
    key_id: Option<String>,
}

fn extract_call_meta(args: &mut Option<Map<String, Value>>) -> CallMeta {
    let mut meta = CallMeta::default();
    if let Some(map) = args.as_mut() {
        if let Some(value) = map.remove("_meta") {
            if let Some(obj) = value.as_object() {
                if let Some(run_id) = obj.get("run_id").and_then(|v| v.as_str()) {
                    meta.run_id = Some(run_id.to_string());
                }
                if let Some(request_id) = obj.get("request_id").and_then(|v| v.as_str()) {
                    meta.request_id = Some(request_id.to_string());
                }
                if let Some(key_id) = obj.get("key_id").and_then(|v| v.as_str()) {
                    meta.key_id = Some(key_id.to_string());
                }
            }
        }
    }
    meta
}

fn merge_meta(existing: Option<Meta>, additions: Value) -> Option<Meta> {
    let mut map = existing.map(|m| m.0).unwrap_or_default();
    if let Value::Object(add) = additions {
        for (k, v) in add {
            map.insert(k, v);
        }
    }
    Some(Meta(map))
}

fn new_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let ts = chrono::Utc::now().timestamp_millis();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    format!("{}-{}-{}-{}", prefix, ts, pid, seq)
}
