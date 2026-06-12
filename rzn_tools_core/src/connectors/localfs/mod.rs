use async_trait::async_trait;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;
use crate::Connector;
use htmd::HtmlToMarkdown;
use rmcp::model::*;

mod extractors;
mod types;

pub use extractors::{detect_file_type, get_extractor_for_path, Extractor};
pub use types::*;

fn html_to_markdown(html: &str) -> String {
    let converter = HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "nav", "footer", "header", "aside", "img", "a", "href", "src",
        ])
        .build();
    converter.convert(html).unwrap_or_else(|_| html.to_string())
}

fn truncate_to_chars(s: &str, max_chars: usize) -> (String, bool) {
    if max_chars == 0 {
        return (String::new(), !s.is_empty());
    }
    if s.chars().count() <= max_chars {
        return (s.to_string(), false);
    }
    let mut end = 0;
    for (i, (idx, _)) in s.char_indices().enumerate() {
        if i == max_chars {
            end = idx;
            break;
        }
    }
    (s[..end].to_string(), true)
}

/// Expand `~` to the user's home directory
fn expand_path(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

#[derive(Clone)]
pub struct LocalFsConnector;

impl Default for LocalFsConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalFsConnector {
    pub fn new() -> Self {
        LocalFsConnector
    }

    async fn list_files(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let path =
            args.get("path")
                .and_then(|v| v.as_str())
                .ok_or(ConnectorError::InvalidParams(
                    "Missing 'path' parameter".to_string(),
                ))?;

        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let extensions: Option<Vec<&str>> = args
            .get("extensions")
            .and_then(|v| v.as_str())
            .map(|s| s.split(',').map(|e| e.trim()).collect());

        let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(100) as usize;

        let dir_path = expand_path(path);
        if !dir_path.is_dir() {
            return Err(ConnectorError::InvalidParams(format!(
                "Path is not a directory: {}",
                dir_path.display()
            )));
        }

        let mut files = Vec::new();
        let mut total_count = 0;

        fn visit_dir(
            dir: &Path,
            recursive: bool,
            extensions: &Option<Vec<&str>>,
            limit: usize,
            files: &mut Vec<FileInfo>,
            total_count: &mut usize,
        ) -> std::io::Result<()> {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_file() {
                    // Check extension filter
                    if let Some(ref exts) = extensions {
                        let file_ext = path
                            .extension()
                            .and_then(|e| e.to_str())
                            .map(|e| e.to_lowercase());

                        if let Some(ext) = file_ext {
                            if !exts.iter().any(|e| e.to_lowercase() == ext) {
                                continue;
                            }
                        } else {
                            continue; // No extension, skip
                        }
                    }

                    *total_count += 1;

                    if files.len() < limit {
                        let metadata = std::fs::metadata(&path)?;
                        let file_type = detect_file_type(&path);

                        files.push(FileInfo {
                            path: path.to_string_lossy().to_string(),
                            name: path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string(),
                            extension: path
                                .extension()
                                .and_then(|e| e.to_str())
                                .map(|s| s.to_string()),
                            size_bytes: metadata.len(),
                            modified: metadata.modified().ok().map(|t| {
                                chrono::DateTime::<chrono::Utc>::from(t)
                                    .format("%Y-%m-%dT%H:%M:%S%:z")
                                    .to_string()
                            }),
                            file_type,
                            mime_type: None,
                        });
                    }
                } else if path.is_dir() && recursive {
                    visit_dir(&path, recursive, extensions, limit, files, total_count)?;
                }
            }
            Ok(())
        }

        visit_dir(
            &dir_path,
            recursive,
            &extensions,
            limit,
            &mut files,
            &mut total_count,
        )
        .map_err(|e| ConnectorError::Other(format!("Failed to read directory: {}", e)))?;

        let result = FileListResult {
            directory: path.to_string(),
            total_count,
            truncated: total_count > limit,
            files,
        };

        let text = serde_json::to_string(&result)?;
        structured_result_with_text(&result, Some(text))
    }

    async fn get_file_info(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let path =
            args.get("path")
                .and_then(|v| v.as_str())
                .ok_or(ConnectorError::InvalidParams(
                    "Missing 'path' parameter".to_string(),
                ))?;

        let path_obj = expand_path(path);
        let metadata = std::fs::metadata(&path_obj)
            .map_err(|e| ConnectorError::Other(format!("Failed to get file metadata: {}", e)))?;

        let file_type = detect_file_type(&path_obj);

        let file_info = FileInfo {
            path: path_obj.to_string_lossy().to_string(),
            name: path_obj
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string(),
            extension: path_obj
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_string()),
            size_bytes: metadata.len(),
            modified: metadata
                .modified()
                .ok()
                .and_then(|t| {
                    t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| {
                        chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                            .map(|dt| dt.to_rfc3339())
                    })
                })
                .flatten(),
            file_type,
            mime_type: None,
        };

        let text = serde_json::to_string(&file_info)?;
        structured_result_with_text(&file_info, Some(text))
    }

    async fn extract_text(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let path =
            args.get("path")
                .and_then(|v| v.as_str())
                .ok_or(ConnectorError::InvalidParams(
                    "Missing 'path' parameter".to_string(),
                ))?;

        let format = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("plain");
        let max_chars: Option<usize> = args
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        let path_obj = expand_path(path);
        let ext = path_obj
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let mut text_content: types::TextContent =
            if format == "markdown" && matches!(ext.as_str(), "html" | "htm" | "xhtml") {
                let html = std::fs::read_to_string(&path_obj)
                    .map_err(|e| ConnectorError::Other(format!("Failed to read file: {}", e)))?;
                let content = html_to_markdown(&html);
                let word_count = content.split_whitespace().count();
                let char_count = content.chars().count();
                types::TextContent {
                    path: path_obj.to_string_lossy().to_string(),
                    content,
                    format: "markdown".to_string(),
                    word_count,
                    char_count,
                    truncated: false,
                    original_char_count: None,
                }
            } else {
                let extractor = get_extractor_for_path(&path_obj).ok_or(ConnectorError::Other(
                    format!("Unsupported file type: {}", path_obj.display()),
                ))?;
                let mut tc: types::TextContent = extractor.extract_text(&path_obj)?;
                // Honor requested format (even if identical content for most file types).
                tc.format = format.to_string();
                tc
            };

        if let Some(limit) = max_chars {
            let original = text_content.char_count;
            let (truncated, did_truncate) = truncate_to_chars(&text_content.content, limit);
            if did_truncate {
                text_content.content = truncated;
                text_content.original_char_count = Some(original);
                text_content.char_count = text_content.content.chars().count();
                text_content.word_count = text_content.content.split_whitespace().count();
                text_content.truncated = true;
            }
        }

        let text = serde_json::to_string(&text_content)?;
        structured_result_with_text(&text_content, Some(text))
    }

    async fn get_structure(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let path =
            args.get("path")
                .and_then(|v| v.as_str())
                .ok_or(ConnectorError::InvalidParams(
                    "Missing 'path' parameter".to_string(),
                ))?;

        let path_obj = expand_path(path);
        let extractor = get_extractor_for_path(&path_obj).ok_or(ConnectorError::Other(format!(
            "Unsupported file type: {}",
            path_obj.display()
        )))?;

        let structure = extractor.get_structure(&path_obj)?;
        let text = serde_json::to_string(&structure)?;
        structured_result_with_text(&structure, Some(text))
    }

    async fn get_section(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let path =
            args.get("path")
                .and_then(|v| v.as_str())
                .ok_or(ConnectorError::InvalidParams(
                    "Missing 'path' parameter".to_string(),
                ))?;

        let section =
            args.get("section")
                .and_then(|v| v.as_str())
                .ok_or(ConnectorError::InvalidParams(
                    "Missing 'section' parameter".to_string(),
                ))?;

        let path_obj = expand_path(path);
        let extractor = get_extractor_for_path(&path_obj).ok_or(ConnectorError::Other(format!(
            "Unsupported file type: {}",
            path_obj.display()
        )))?;

        let mut section_content = extractor.get_section(&path_obj, section)?;

        let max_chars: Option<usize> = args
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        if let Some(limit) = max_chars {
            let original = section_content.content.chars().count();
            let (truncated, did_truncate) = truncate_to_chars(&section_content.content, limit);
            if did_truncate {
                section_content.content = truncated;
                section_content.original_char_count = Some(original);
                section_content.word_count = section_content.content.split_whitespace().count();
                section_content.truncated = true;
            }
        }

        let text = serde_json::to_string(&section_content)?;
        structured_result_with_text(&section_content, Some(text))
    }

    async fn search_content(
        &self,
        args: &serde_json::Map<String, Value>,
    ) -> Result<CallToolResult, ConnectorError> {
        let path =
            args.get("path")
                .and_then(|v| v.as_str())
                .ok_or(ConnectorError::InvalidParams(
                    "Missing 'path' parameter".to_string(),
                ))?;

        let query =
            args.get("query")
                .and_then(|v| v.as_str())
                .ok_or(ConnectorError::InvalidParams(
                    "Missing 'query' parameter".to_string(),
                ))?;

        let context_lines = args
            .get("context_lines")
            .and_then(|v| v.as_i64())
            .unwrap_or(2) as usize;

        let path_obj = expand_path(path);
        let extractor = get_extractor_for_path(&path_obj).ok_or(ConnectorError::Other(format!(
            "Unsupported file type: {}",
            path_obj.display()
        )))?;

        let search_result = extractor.search(&path_obj, query, context_lines)?;
        let text = serde_json::to_string(&search_result)?;
        structured_result_with_text(&search_result, Some(text))
    }
}

#[async_trait]
impl Connector for LocalFsConnector {
    fn name(&self) -> &'static str {
        "localfs"
    }

    fn description(&self) -> &'static str {
        "Local filesystem text extraction connector for PDF, EPUB, DOCX, HTML, Markdown, code, and text files"
    }

    fn display_name(&self) -> &'static str {
        "Local Files"
    }

    fn icon(&self) -> &'static str {
        "localfs"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["files", "local", "storage"]
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
        // No auth required for local filesystem
        Ok(())
    }

    async fn test_auth(&self) -> Result<(), ConnectorError> {
        // No auth required
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
                title: None,
                version: "0.1.0".to_string(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Local filesystem connector for extracting text from documents. Supports PDF, EPUB, DOCX, HTML, Markdown, code, and text files.".to_string(),
            ),
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
                name: Cow::Borrowed("list_files"),
                title: None,
                description: Some(Cow::Borrowed(
                    "List files in a directory. Use when you need candidate paths to extract. \
Example: path=\"~/Downloads\" recursive=false extensions=\"pdf,md\" limit=50.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Directory path to list"
                            },
                            "recursive": {
                                "type": "boolean",
                                "description": "Recurse into subdirectories",
                                "default": false
                            },
                            "extensions": {
                                "type": "string",
                                "description": "Comma-separated list of extensions to filter (e.g., 'pdf,epub,md')"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum number of files to return",
                                "default": 100
                            }
                        },
                        "required": ["path"]
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
                name: Cow::Borrowed("get_file_info"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get file metadata by path. Use when you need size/type/modified time before extracting.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path"
                            }
                        },
                        "required": ["path"]
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
                name: Cow::Borrowed("extract_text"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Extract text from a local file. Use format=\"markdown\" for HTML files \
(best-effort conversion). Tip: set max_chars to avoid huge outputs. Example: path=\"~/doc.pdf\" max_chars=8000.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path"
                            },
                            "format": {
                                "type": "string",
                                "description": "Output format - 'plain' (default) or 'markdown'",
                                "default": "plain",
                                "enum": ["plain", "markdown"]
                            },
                            "max_chars": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Optional max characters to return (truncate content)."
                            }
                        },
                        "required": ["path"]
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
                name: Cow::Borrowed("get_structure"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get document structure (TOC/headings) so you can request targeted sections. \
Example: path=\"~/paper.pdf\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path"
                            }
                        },
                        "required": ["path"]
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
                name: Cow::Borrowed("get_section"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Get a specific section by identifier (use get_structure to discover IDs). \
Tip: set max_chars to keep responses small. Example: path=\"~/book.epub\" section=\"chapter:3\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path"
                            },
                            "section": {
                                "type": "string",
                                "description": "Section identifier (e.g., 'page:5', 'chapter:3', 'heading:2', 'lines:10-50')"
                            },
                            "max_chars": {
                                "type": "integer",
                                "minimum": 1,
                                "description": "Optional max characters to return (truncate content)."
                            }
                        },
                        "required": ["path", "section"]
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
                name: Cow::Borrowed("search_content"),
                title: None,
                description: Some(Cow::Borrowed(
                    "Search within a file. Use when you need matching snippets, not the whole \
document. Example: path=\"~/spec.pdf\" query=\"threat model\" context_lines=2.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path"
                            },
                            "query": {
                                "type": "string",
                                "description": "Search query"
                            },
                            "context_lines": {
                                "type": "integer",
                                "description": "Lines of context around matches",
                                "default": 2
                            }
                        },
                        "required": ["path", "query"]
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
        request: CallToolRequestParam,
    ) -> Result<CallToolResult, ConnectorError> {
        let name = request.name.as_ref();
        let args = request.arguments.unwrap_or_default();

        match name {
            "list_files" => self.list_files(&args).await,
            "get_file_info" => self.get_file_info(&args).await,
            "extract_text" => self.extract_text(&args).await,
            "get_structure" => self.get_structure(&args).await,
            "get_section" => self.get_section(&args).await,
            "search_content" => self.search_content(&args).await,
            _ => Err(ConnectorError::ToolNotFound),
        }
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
        Err(ConnectorError::InvalidParams(
            "Prompts not supported".to_string(),
        ))
    }
}
