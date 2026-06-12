// Remove async_mcp reference, use standard JSON-RPC error codes
// src/error.rs
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serde JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Resource not found")]
    ResourceNotFound,

    #[error("Tool not found")]
    ToolNotFound,

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Invalid params: {0}")]
    InvalidParams(String),

    #[error("Method not found")]
    MethodNotFound,

    #[error("Parse error")]
    ParseError,

    #[error("Other error: {0}")]
    Other(String),

    #[error("HTTP request error: {0}")]
    HttpRequest(#[from] reqwest::Error),

    #[error("Twitter scraper error: {0}")]
    TwitterScraper(String),

    #[error("Page is a CAPTCHA or authentication challenge")]
    PageIsCaptchaOrAuthChallenge,

    #[error("Timeout: {0}")]
    Timeout(String),
}

impl ConnectorError {
    pub fn code_str(&self) -> &'static str {
        match self {
            ConnectorError::InvalidInput(_) => "invalid_input",
            ConnectorError::InvalidParams(_) => "invalid_params",
            ConnectorError::Authentication(_) => "auth_failed",
            ConnectorError::ResourceNotFound => "not_found",
            ConnectorError::ToolNotFound => "tool_not_found",
            ConnectorError::MethodNotFound => "method_not_found",
            ConnectorError::ParseError => "parse_error",
            ConnectorError::Timeout(_) => "timeout",
            ConnectorError::HttpRequest(_) => "upstream_error",
            ConnectorError::TwitterScraper(_) => "upstream_error",
            ConnectorError::PageIsCaptchaOrAuthChallenge => "blocked",
            ConnectorError::InternalError(_) => "internal_error",
            ConnectorError::Other(_) => "internal_error",
            _ => "internal_error",
        }
    }
    pub fn to_jsonrpc_error(&self) -> serde_json::Value {
        let (code, message) = match self {
            ConnectorError::ResourceNotFound => (-32602, "Resource not found".to_string()),
            ConnectorError::ToolNotFound => (-32602, "Tool not found".to_string()),
            ConnectorError::InternalError(msg) => (-32603, msg.to_string()),
            ConnectorError::InvalidParams(msg) => (-32602, msg.to_string()),
            ConnectorError::InvalidInput(msg) => (-32602, msg.to_string()),
            ConnectorError::MethodNotFound => (-32601, "Method not found".to_string()),
            ConnectorError::ParseError => (-32700, "Parse error".to_string()),
            ConnectorError::Other(msg) => (-32603, msg.to_string()),
            err => (-32603, err.to_string()),
        };

        json!({
            "code": code,
            "message": message,
        })
    }
}
