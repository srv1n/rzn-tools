// Apple Common - Shared infrastructure for Apple ecosystem connectors
// Provides AppleScript execution, output parsing, and common utilities
// macOS only

use serde::{Deserialize, Serialize};
use std::process::Stdio;

use crate::error::ConnectorError;

/// Result of running an AppleScript
#[derive(Debug, Clone)]
pub struct ScriptResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ScriptResult {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Execute an AppleScript and return the result
#[cfg(target_os = "macos")]
pub async fn run_applescript(script: &str) -> Result<ScriptResult, ConnectorError> {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut cmd = Command::new("/usr/bin/osascript");
    cmd.arg("-s").arg("s"); // structured output
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| ConnectorError::Other(format!("Failed to spawn osascript: {}", e)))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| ConnectorError::Other(format!("Failed to write script: {}", e)))?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| ConnectorError::Other(format!("Failed to wait for osascript: {}", e)))?;

    Ok(ScriptResult {
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

#[cfg(not(target_os = "macos"))]
pub async fn run_applescript(_script: &str) -> Result<ScriptResult, ConnectorError> {
    Err(ConnectorError::Other(
        "AppleScript is only available on macOS".to_string(),
    ))
}

/// Execute AppleScript and return stdout, or error if failed
#[cfg(target_os = "macos")]
pub async fn run_applescript_output(script: &str) -> Result<String, ConnectorError> {
    let result = run_applescript(script).await?;
    if result.success() {
        Ok(result.stdout)
    } else {
        Err(ConnectorError::Other(format!(
            "AppleScript error: {}",
            result.stderr
        )))
    }
}

#[cfg(not(target_os = "macos"))]
pub async fn run_applescript_output(_script: &str) -> Result<String, ConnectorError> {
    Err(ConnectorError::Other(
        "AppleScript is only available on macOS".to_string(),
    ))
}

/// Escape a string for use in AppleScript
pub fn escape_applescript_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Parse AppleScript list output into a Vec of strings
/// AppleScript returns lists like: {"item1", "item2", "item3"}
pub fn parse_applescript_list(output: &str) -> Vec<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        return Vec::new();
    }

    // Remove outer braces
    let inner = if trimmed.starts_with('{') && trimmed.ends_with('}') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    // Split by ", " but handle quoted strings
    let mut items = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = inner.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
            }
            ',' if !in_quotes => {
                let item = current.trim().trim_matches('"').to_string();
                if !item.is_empty() {
                    items.push(item);
                }
                current.clear();
                // Skip space after comma
                if chars.peek() == Some(&' ') {
                    chars.next();
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    // Don't forget the last item
    let item = current.trim().trim_matches('"').to_string();
    if !item.is_empty() {
        items.push(item);
    }

    items
}

/// Parse AppleScript record output into key-value pairs
/// AppleScript returns records like: {name:"value", id:123}
pub fn parse_applescript_record(output: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let trimmed = output.trim();

    if trimmed.is_empty() || trimmed == "{}" {
        return map;
    }

    // Remove outer braces
    let inner = if trimmed.starts_with('{') && trimmed.ends_with('}') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    // Simple key:value parsing
    for part in inner.split(", ") {
        if let Some(colon_pos) = part.find(':') {
            let key = part[..colon_pos].trim().to_string();
            let value = part[colon_pos + 1..].trim().trim_matches('"').to_string();
            map.insert(key, value);
        }
    }

    map
}

/// Common date format for Apple apps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppleDate {
    pub date_string: String,
    pub timestamp: Option<i64>,
}

impl AppleDate {
    pub fn from_applescript(date_str: &str) -> Self {
        // AppleScript dates come in various formats
        // We'll store the string and try to parse timestamp if possible
        Self {
            date_string: date_str.to_string(),
            timestamp: None, // Could add parsing later
        }
    }
}

/// Check if an app is running
#[cfg(target_os = "macos")]
pub async fn is_app_running(app_name: &str) -> Result<bool, ConnectorError> {
    let script = format!(
        r#"tell application "System Events" to (name of processes) contains "{}""#,
        escape_applescript_string(app_name)
    );
    let result = run_applescript_output(&script).await?;
    Ok(result.trim() == "true")
}

#[cfg(not(target_os = "macos"))]
pub async fn is_app_running(_app_name: &str) -> Result<bool, ConnectorError> {
    Err(ConnectorError::Other(
        "App check is only available on macOS".to_string(),
    ))
}

/// Standard connector capabilities for Apple connectors
pub fn apple_connector_capabilities() -> rmcp::model::ServerCapabilities {
    rmcp::model::ServerCapabilities {
        tools: Some(rmcp::model::ToolsCapability { list_changed: None }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_applescript_list() {
        let output = r#"{"item1", "item2", "item3"}"#;
        let items = parse_applescript_list(output);
        assert_eq!(items, vec!["item1", "item2", "item3"]);
    }

    #[test]
    fn test_parse_empty_list() {
        let output = "{}";
        let items = parse_applescript_list(output);
        assert!(items.is_empty());
    }

    #[test]
    fn test_escape_applescript_string() {
        let input = r#"Hello "World""#;
        let escaped = escape_applescript_string(input);
        assert_eq!(escaped, r#"Hello \"World\""#);
    }
}
