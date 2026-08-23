// src/connectors/spotlight/mod.rs
// macOS Spotlight search connector using mdfind CLI
// Provides programmatic access to Spotlight-indexed content

use async_trait::async_trait;
use rmcp::model::*;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::Arc;

use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;

/// macOS Spotlight connector for searching indexed files and content.
///
/// Uses the `mdfind` CLI which queries Spotlight's NSMetadataQuery under the hood.
/// Can search by:
/// - Content (full-text search in documents)
/// - File name
/// - File type/kind
/// - Metadata attributes
/// - Date ranges
///
/// Only available on macOS.
#[derive(Default)]
pub struct SpotlightConnector;

impl SpotlightConnector {
    pub fn new() -> Self {
        Self {}
    }

    /// Run mdfind with the given query and options
    #[cfg(target_os = "macos")]
    async fn run_mdfind(
        &self,
        query: &str,
        only_in: Option<&str>,
        limit: Option<usize>,
        name_only: bool,
    ) -> Result<Vec<String>, ConnectorError> {
        use tokio::process::Command;

        let mut cmd = Command::new("/usr/bin/mdfind");

        if name_only {
            cmd.arg("-name");
        }

        if let Some(dir) = only_in {
            cmd.arg("-onlyin").arg(dir);
        }

        cmd.arg(query);

        let output = cmd
            .output()
            .await
            .map_err(|e| ConnectorError::Other(format!("Failed to run mdfind: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectorError::Other(format!("mdfind failed: {}", stderr)));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut results: Vec<String> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .map(|s| s.to_string())
            .collect();

        if let Some(max) = limit {
            results.truncate(max);
        }

        Ok(results)
    }

    #[cfg(not(target_os = "macos"))]
    async fn run_mdfind(
        &self,
        _query: &str,
        _only_in: Option<&str>,
        _limit: Option<usize>,
        _name_only: bool,
    ) -> Result<Vec<String>, ConnectorError> {
        Err(ConnectorError::Other(
            "Spotlight search is only available on macOS".to_string(),
        ))
    }

    /// Get metadata for a file using mdls
    #[cfg(target_os = "macos")]
    async fn get_file_metadata(&self, path: &str) -> Result<Value, ConnectorError> {
        use tokio::process::Command;

        let output = Command::new("/usr/bin/mdls")
            .arg("-plist")
            .arg("-")
            .arg(path)
            .output()
            .await
            .map_err(|e| ConnectorError::Other(format!("Failed to run mdls: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectorError::Other(format!("mdls failed: {}", stderr)));
        }

        // Parse plist output - for simplicity, use the raw format instead
        let output = Command::new("/usr/bin/mdls")
            .arg(path)
            .output()
            .await
            .map_err(|e| ConnectorError::Other(format!("Failed to run mdls: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse the key = value format from mdls
        let mut metadata = serde_json::Map::new();
        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once(" = ") {
                let key = key.trim();
                let value = value.trim();

                // Skip null values
                if value == "(null)" {
                    continue;
                }

                // Parse value type
                let parsed_value = if value.starts_with('"') && value.ends_with('"') {
                    // String value
                    Value::String(value[1..value.len() - 1].to_string())
                } else if value == "1" || value == "0" {
                    // Boolean-ish
                    Value::Number(value.parse::<i64>().unwrap_or(0).into())
                } else if let Ok(num) = value.parse::<i64>() {
                    Value::Number(num.into())
                } else if let Ok(num) = value.parse::<f64>() {
                    Value::Number(serde_json::Number::from_f64(num).unwrap_or(0.into()))
                } else if value.starts_with('(') && value.ends_with(')') {
                    // Array - simplified parsing
                    Value::String(value.to_string())
                } else {
                    Value::String(value.to_string())
                };

                metadata.insert(key.to_string(), parsed_value);
            }
        }

        Ok(Value::Object(metadata))
    }

    #[cfg(not(target_os = "macos"))]
    async fn get_file_metadata(&self, _path: &str) -> Result<Value, ConnectorError> {
        Err(ConnectorError::Other(
            "File metadata is only available on macOS".to_string(),
        ))
    }

    /// Build a Spotlight query from structured parameters
    fn build_query(
        &self,
        content: Option<&str>,
        kind: Option<&str>,
        author: Option<&str>,
        date_from: Option<&str>,
        date_to: Option<&str>,
    ) -> String {
        let mut parts = Vec::new();

        if let Some(content) = content {
            // Full-text content search
            parts.push(format!("kMDItemTextContent == \"*{}*\"cd", content));
        }

        if let Some(kind) = kind {
            // Map common kinds to Spotlight types
            let kind_query = match kind.to_lowercase().as_str() {
                "pdf" => "kMDItemContentType == \"com.adobe.pdf\"",
                "image" | "images" => "kMDItemContentTypeTree == \"public.image\"",
                "video" | "videos" => "kMDItemContentTypeTree == \"public.movie\"",
                "audio" | "music" => "kMDItemContentTypeTree == \"public.audio\"",
                "document" | "documents" => "kMDItemContentTypeTree == \"public.content\"",
                "email" | "emails" => "kMDItemContentType == \"com.apple.mail.emlx\"",
                "presentation" | "presentations" => "kMDItemContentType == \"com.apple.keynote.key\" || kMDItemContentType == \"org.openxmlformats.presentationml.presentation\" || kMDItemContentType == \"com.microsoft.powerpoint.ppt\"",
                "spreadsheet" | "spreadsheets" => "kMDItemContentType == \"com.apple.numbers.numbers\" || kMDItemContentType == \"org.openxmlformats.spreadsheetml.sheet\" || kMDItemContentType == \"com.microsoft.excel.xls\"",
                "code" | "source" => "kMDItemContentTypeTree == \"public.source-code\"",
                "text" => "kMDItemContentTypeTree == \"public.plain-text\"",
                "folder" | "directory" => "kMDItemContentType == \"public.folder\"",
                "application" | "app" => "kMDItemContentType == \"com.apple.application-bundle\"",
                "markdown" | "md" => "kMDItemContentType == \"net.daringfireball.markdown\"",
                _ => {
                    // Use as-is if it looks like a UTI, otherwise search display name
                    if kind.contains('.') {
                        return format!("kMDItemContentType == \"{}\"", kind);
                    } else {
                        return format!("kMDItemKind == \"*{}*\"cd", kind);
                    }
                }
            };
            parts.push(kind_query.to_string());
        }

        if let Some(author) = author {
            parts.push(format!("kMDItemAuthors == \"*{}*\"cd", author));
        }

        if let Some(from) = date_from {
            parts.push(format!(
                "kMDItemContentModificationDate >= $time.iso({})",
                from
            ));
        }

        if let Some(to) = date_to {
            parts.push(format!(
                "kMDItemContentModificationDate <= $time.iso({})",
                to
            ));
        }

        if parts.is_empty() {
            "*".to_string() // Match all
        } else {
            parts.join(" && ")
        }
    }
}

#[async_trait]
impl crate::Connector for SpotlightConnector {
    fn name(&self) -> &'static str {
        "spotlight"
    }

    fn description(&self) -> &'static str {
        "macOS Spotlight search connector. Search files by content, name, type, or metadata. \
         Indexes documents, emails, source code, images, and more. Only available on macOS."
    }

    fn display_name(&self) -> &'static str {
        "Spotlight"
    }

    fn icon(&self) -> &'static str {
        "spotlight"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["search", "local", "files"]
    }

    fn requires_auth(&self) -> bool {
        false
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // Test by running a simple query
        #[cfg(target_os = "macos")]
        {
            self.run_mdfind("kMDItemDisplayName == 'test'", None, Some(1), false)
                .await?;
        }
        Ok(())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
    ) -> Result<ListToolsResult, ConnectorError> {
        // Keep the surface small to reduce ambiguity and context bloat for agents.
        let tools = vec![
            Tool {
                name: Cow::Borrowed("search"),
                title: Some("Search Spotlight".to_string()),
                description: Some(Cow::Borrowed(
                    "Search Spotlight index by content/name/kind/recent/raw. Use mode to choose \
the search type. Example: mode=\"content\" query=\"invoice\" directory=\"~/Documents\" limit=20.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "mode": {
                                "type": "string",
                                "enum": ["content", "name", "kind", "recent", "raw"],
                                "description": "Search mode. Use 'content' for full-text, 'name' for file names, 'kind' for file types, 'recent' for modified files, 'raw' for mdfind syntax.",
                                "default": "content"
                            },
                            "query": {
                                "type": "string",
                                "description": "Search query text. Required for mode=content/name/raw."
                            },
                            "directory": {
                                "type": "string",
                                "description": "Optional: limit search to this directory"
                            },
                            "kind": {
                                "type": "string",
                                "description": "File type filter for mode=content/recent OR required file type for mode=kind.",
                                "enum": [
                                    "pdf",
                                    "image",
                                    "video",
                                    "audio",
                                    "document",
                                    "email",
                                    "code",
                                    "text",
                                    "markdown",
                                    "spreadsheet",
                                    "presentation",
                                    "application",
                                    "folder"
                                ]
                            },
                            "days": {
                                "type": "integer",
                                "description": "Only for mode=recent: modified within N days (default: 7).",
                                "default": 7
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum number of results (default: 50)",
                                "default": 50
                            }
                        },
                        "required": []
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
                name: Cow::Borrowed("get_metadata"),
                title: Some("Get File Metadata".to_string()),
                description: Some(Cow::Borrowed(
                    "Get Spotlight metadata for a file path. Use when you already have a path \
and want its indexed attributes.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Full path to the file"
                            }
                        },
                        "required": ["path"]
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

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();

        match name {
            "search" => {
                let mode = args
                    .get("mode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("content");
                let directory = args.get("directory").and_then(|v| v.as_str());
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize)
                    .unwrap_or(50);

                match mode {
                    "content" => {
                        let query_text =
                            args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                                ConnectorError::InvalidInput(
                                    "Missing 'query' for mode=content".into(),
                                )
                            })?;
                        let kind = args.get("kind").and_then(|v| v.as_str());
                        let query = self.build_query(Some(query_text), kind, None, None, None);
                        let results = self
                            .run_mdfind(&query, directory, Some(limit), false)
                            .await?;
                        let payload = json!({
                            "query": query_text,
                            "spotlight_query": query,
                            "directory": directory,
                            "count": results.len(),
                            "files": results
                        });
                        structured_result_with_text(&payload, None)
                    }
                    "name" => {
                        let name_query =
                            args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                                ConnectorError::InvalidInput("Missing 'query' for mode=name".into())
                            })?;
                        let results = self
                            .run_mdfind(name_query, directory, Some(limit), true)
                            .await?;
                        let payload = json!({
                            "name_query": name_query,
                            "directory": directory,
                            "count": results.len(),
                            "files": results
                        });
                        structured_result_with_text(&payload, None)
                    }
                    "kind" => {
                        let kind = args.get("kind").and_then(|v| v.as_str()).ok_or_else(|| {
                            ConnectorError::InvalidInput("Missing 'kind' for mode=kind".into())
                        })?;
                        let query = self.build_query(None, Some(kind), None, None, None);
                        let results = self
                            .run_mdfind(&query, directory, Some(limit), false)
                            .await?;
                        let payload = json!({
                            "kind": kind,
                            "spotlight_query": query,
                            "directory": directory,
                            "count": results.len(),
                            "files": results
                        });
                        structured_result_with_text(&payload, None)
                    }
                    "recent" => {
                        let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(7) as i64;
                        let kind = args.get("kind").and_then(|v| v.as_str());
                        let mut query_parts = vec![format!(
                            "kMDItemContentModificationDate >= $time.today(-{})",
                            days
                        )];
                        if let Some(kind) = kind {
                            query_parts.push(self.build_query(None, Some(kind), None, None, None));
                        }
                        let query = query_parts.join(" && ");
                        let results = self
                            .run_mdfind(&query, directory, Some(limit), false)
                            .await?;
                        let payload = json!({
                            "days": days,
                            "kind": kind,
                            "spotlight_query": query,
                            "directory": directory,
                            "count": results.len(),
                            "files": results
                        });
                        structured_result_with_text(&payload, None)
                    }
                    "raw" => {
                        let query =
                            args.get("query").and_then(|v| v.as_str()).ok_or_else(|| {
                                ConnectorError::InvalidInput("Missing 'query' for mode=raw".into())
                            })?;
                        let results = self
                            .run_mdfind(query, directory, Some(limit), false)
                            .await?;
                        let payload = json!({
                            "query": query,
                            "directory": directory,
                            "count": results.len(),
                            "files": results
                        });
                        structured_result_with_text(&payload, None)
                    }
                    _ => Err(ConnectorError::InvalidInput(format!(
                        "Invalid 'mode': {}",
                        mode
                    ))),
                }
            }
            "get_metadata" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ConnectorError::InvalidInput("Missing 'path'".to_string()))?;
                let metadata = self.get_file_metadata(path).await?;
                let payload = json!({
                    "path": path,
                    "metadata": metadata
                });
                structured_result_with_text(&payload, None)
            }
            _ => Err(ConnectorError::ToolNotFound),
        }
    }
}
