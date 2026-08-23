use std::{
    collections::HashSet,
    convert::Infallible,
    io,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Bytes,
    extract::State,
    http::{
        header::{HeaderName, HOST},
        HeaderMap, HeaderValue, Method, StatusCode,
    },
    response::{
        sse::{Event, Sse},
        IntoResponse, Response,
    },
    routing::{any, get},
    Router,
};
use futures::stream;
use serde_json::{json, Map, Value};
use tokio::sync::RwLock;
use tracing::{error, info};

use rzn_tools_core::mcp_server::JsonRpcHandler;

const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";
const CACHE_CONTROL_HEADER: &str = "cache-control";
const PRAGMA_HEADER: &str = "pragma";
const HTTP_NO_STORE: &str = "no-store, max-age=0";
const DEFAULT_BIND_ADDR: &str = "127.0.0.1:8000";

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub bind: SocketAddr,
    pub allowed_hosts: Option<HashSet<String>>,
    pub exposed_connectors: Option<HashSet<String>>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND_ADDR
                .parse()
                .expect("default bind address is valid"),
            allowed_hosts: None,
            exposed_connectors: None,
        }
    }
}

#[derive(Clone)]
pub struct HttpServer {
    state: AppState,
}

impl HttpServer {
    pub fn new(handler: JsonRpcHandler, config: HttpConfig) -> Self {
        Self {
            state: AppState {
                handler: Arc::new(handler),
                sessions: Arc::new(RwLock::new(HashSet::new())),
                cached_init: Arc::new(RwLock::new(None)),
                bind: config.bind,
                allowed_hosts: Arc::new(config.allowed_hosts),
            },
        }
    }

    pub async fn warm_up(&self) -> io::Result<()> {
        let initialize = json!({
            "jsonrpc": "2.0",
            "id": "__http_init__",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "rzn-tools-mcp/http",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        let init_response = self.state.handler.handle_request(initialize).await;
        let Some(result) = init_response
            .get("result")
            .and_then(Value::as_object)
            .cloned()
        else {
            return Err(io::Error::other(format!(
                "initialize warmup failed: {}",
                init_response
            )));
        };
        *self.state.cached_init.write().await = Some(result);

        Ok(())
    }

    pub async fn serve(self) -> io::Result<()> {
        let router = Router::new()
            .route("/", any(handle_mcp))
            .route("/mcp", any(handle_mcp))
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz))
            .fallback(not_found)
            .with_state(self.state.clone());
        let listener = tokio::net::TcpListener::bind(self.state.bind).await?;

        info!(
            "MCP HTTP server listening on http://{}/mcp",
            self.state.bind
        );

        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .map_err(io::Error::other)
    }
}

#[derive(Clone)]
struct AppState {
    handler: Arc<JsonRpcHandler>,
    sessions: Arc<RwLock<HashSet<String>>>,
    cached_init: Arc<RwLock<Option<Map<String, Value>>>>,
    bind: SocketAddr,
    allowed_hosts: Arc<Option<HashSet<String>>>,
}

async fn healthz() -> impl IntoResponse {
    (
        StatusCode::OK,
        axum::Json(json!({
            "ok": true
        })),
    )
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let cached_init = state.cached_init.read().await;

    (
        StatusCode::OK,
        axum::Json(json!({
            "ok": cached_init.is_some(),
            "bind": state.bind.to_string(),
        })),
    )
}

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({
            "error": "Not found"
        })),
    )
}

async fn handle_mcp(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if let Some(response) = reject_host(&state, &headers) {
        return response;
    }

    match method {
        Method::POST => handle_post(state, headers, body).await,
        Method::DELETE => handle_delete(state, headers).await,
        Method::GET => json_or_sse_response(
            StatusCode::METHOD_NOT_ALLOWED,
            json!({ "error": "SSE GET is not supported yet" }),
            false,
            None,
        ),
        _ => json_or_sse_response(
            StatusCode::METHOD_NOT_ALLOWED,
            json!({ "error": "Method not allowed" }),
            false,
            None,
        ),
    }
}

async fn handle_delete(state: AppState, headers: HeaderMap) -> Response {
    if let Some(session_id) = session_id_from_headers(&headers) {
        state.sessions.write().await.remove(&session_id);
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn handle_post(state: AppState, headers: HeaderMap, body: Bytes) -> Response {
    let wants_sse = wants_sse(&headers);

    let request = match parse_request(&body) {
        Ok(request) => request,
        Err(error_response) => {
            return json_or_sse_response(StatusCode::BAD_REQUEST, error_response, wants_sse, None)
        }
    };

    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let msg_id = request.get("id").cloned();
    let is_notification = msg_id.is_none();

    if method == "initialize" {
        let session_id = new_session_id();
        state.sessions.write().await.insert(session_id.clone());

        let result = cached_or_live_initialize(&state, &request).await;
        return json_or_sse_response(StatusCode::OK, result, wants_sse, Some(session_id));
    }

    let session_id = session_id_from_headers(&headers);
    let session_valid = match session_id.as_ref() {
        Some(id) => state.sessions.read().await.contains(id),
        None => false,
    };
    if !session_valid {
        return json_or_sse_response(
            StatusCode::BAD_REQUEST,
            json!({
                "jsonrpc": "2.0",
                "error": {
                    "code": -32600,
                    "message": "Invalid or missing session"
                },
                "id": msg_id
            }),
            wants_sse,
            None,
        );
    }

    if method == "notifications/initialized" {
        return StatusCode::ACCEPTED.into_response();
    }

    if is_notification {
        let _ = state.handler.handle_request(request).await;
        return StatusCode::ACCEPTED.into_response();
    }

    let response = state.handler.handle_request(request).await;
    if method == "tools/list" {
        let response = curate_http_tool_catalog(response);
        return json_or_sse_response(StatusCode::OK, response, wants_sse, None);
    }

    json_or_sse_response(StatusCode::OK, response, wants_sse, None)
}

async fn cached_or_live_initialize(state: &AppState, request: &Value) -> Value {
    if let Some(cached) = state.cached_init.read().await.clone() {
        return json!({
            "jsonrpc": "2.0",
            "result": cached,
            "id": request.get("id").cloned().unwrap_or(Value::Null)
        });
    }

    let response = state.handler.handle_request(request.clone()).await;
    if let Some(result) = response.get("result").and_then(Value::as_object).cloned() {
        *state.cached_init.write().await = Some(result);
    }
    response
}

fn parse_request(body: &[u8]) -> Result<Value, Value> {
    serde_json::from_slice::<Value>(body).map_err(|error| {
        json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32700,
                "message": "Parse error",
                "data": error.to_string()
            },
            "id": null
        })
    })
}

fn json_or_sse_response(
    status: StatusCode,
    payload: Value,
    wants_sse: bool,
    session_id: Option<String>,
) -> Response {
    if wants_sse {
        let event = match serde_json::to_string(&payload) {
            Ok(encoded) => Event::default().event("message").data(encoded),
            Err(error) => {
                error!("failed to serialize SSE payload: {}", error);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({
                        "error": "Failed to serialize SSE payload"
                    })),
                )
                    .into_response();
            }
        };
        let stream = stream::once(async move { Ok::<Event, Infallible>(event) });
        let mut response = Sse::new(stream).into_response();
        *response.status_mut() = status;
        if let Some(session_id) = session_id {
            if let Ok(value) = HeaderValue::from_str(&session_id) {
                response
                    .headers_mut()
                    .insert(HeaderName::from_static(MCP_SESSION_ID_HEADER), value);
            }
        }
        if let Ok(value) = HeaderValue::from_str(HTTP_NO_STORE) {
            response
                .headers_mut()
                .insert(HeaderName::from_static(CACHE_CONTROL_HEADER), value);
        }
        if let Ok(value) = HeaderValue::from_str("no-cache") {
            response
                .headers_mut()
                .insert(HeaderName::from_static(PRAGMA_HEADER), value);
        }
        return response;
    }

    let mut response = (status, axum::Json(payload)).into_response();
    if let Some(session_id) = session_id {
        if let Ok(value) = HeaderValue::from_str(&session_id) {
            response
                .headers_mut()
                .insert(HeaderName::from_static(MCP_SESSION_ID_HEADER), value);
        }
    }
    if let Ok(value) = HeaderValue::from_str(HTTP_NO_STORE) {
        response
            .headers_mut()
            .insert(HeaderName::from_static(CACHE_CONTROL_HEADER), value);
    }
    if let Ok(value) = HeaderValue::from_str("no-cache") {
        response
            .headers_mut()
            .insert(HeaderName::from_static(PRAGMA_HEADER), value);
    }
    response
}

fn reject_host(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let Some(allowed_hosts) = state.allowed_hosts.as_ref() else {
        return None;
    };

    let host = headers
        .get(HOST)
        .and_then(|value| value.to_str().ok())
        .map(host_only)
        .unwrap_or_default();
    if allowed_hosts.contains(&host) {
        return None;
    }

    Some(
        (
            StatusCode::MISDIRECTED_REQUEST,
            axum::Json(json!({
                "error": format!("Host not allowed: {}", host)
            })),
        )
            .into_response(),
    )
}

fn wants_sse(headers: &HeaderMap) -> bool {
    headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"))
}

fn session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn host_only(host: &str) -> String {
    if let Some(stripped) = host.strip_prefix('[') {
        return stripped
            .split(']')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
    }
    if host.matches(':').count() == 1 {
        return host
            .split(':')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
    }
    host.to_ascii_lowercase()
}

fn curate_http_tool_catalog(mut response: Value) -> Value {
    let Some(tools) = response
        .get_mut("result")
        .and_then(|result| result.get_mut("tools"))
        .and_then(Value::as_array_mut)
    else {
        return response;
    };

    let curated = tools.iter().cloned().map(rewrite_http_tool_name).collect();
    *tools = curated;
    response
}

fn rewrite_http_tool_name(mut tool: Value) -> Value {
    let Some(name) = tool
        .get("name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
    else {
        return tool;
    };
    let Some(obj) = tool.as_object_mut() else {
        return tool;
    };
    obj.insert("name".to_string(), Value::String(http_catalog_name(&name)));
    tool
}

fn http_catalog_name(name: &str) -> String {
    name.replace('/', ".")
}

fn new_session_id() -> String {
    let seq = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}{:x}", nanos, seq)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};

        if let Ok(mut signal) = signal(SignalKind::terminate()) {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{curate_http_tool_catalog, host_only, http_catalog_name};
    use serde_json::{json, Value};

    #[test]
    fn strips_simple_host_ports() {
        assert_eq!(host_only("example.com:8787"), "example.com");
        assert_eq!(host_only("example.com"), "example.com");
    }

    #[test]
    fn renders_http_catalog_with_dotted_names() {
        let curated = curate_http_tool_catalog(json!({
            "result": {
                "tools": [
                    { "name": "youtube/get", "description": "Fetch one YouTube video plus transcript/chapters." },
                    { "name": "hackernews/get_thread", "description": "Fetch a Hacker News thread by ID or URL." },
                    { "name": "auth/youtube/set", "description": "Set credentials for youtube." }
                ]
            }
        }));

        let tool_names = curated["result"]["tools"]
            .as_array()
            .expect("tools array")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect::<Vec<_>>();

        assert_eq!(
            tool_names,
            vec!["youtube.get", "hackernews.get_thread", "auth.youtube.set"]
        );
    }

    #[test]
    fn rewrites_http_catalog_names_to_dotted_form() {
        assert_eq!(http_catalog_name("youtube/get"), "youtube.get");
        assert_eq!(
            http_catalog_name("parallel-search/create_monitor"),
            "parallel-search.create_monitor"
        );
    }
}
