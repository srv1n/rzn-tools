// src/connectors/macos/mod.rs
// macOS automation connector: execute AppleScript/JXA and common helpers

use async_trait::async_trait;
use rmcp::model::*;
use serde_json::json;
use std::borrow::Cow;
use std::process::Stdio;
use std::sync::Arc;

use crate::auth::AuthDetails;
use crate::capabilities::ConnectorConfigSchema;
use crate::error::ConnectorError;
use crate::utils::structured_result_with_text;

#[derive(Default)]
pub struct MacOsAutomationConnector;

impl MacOsAutomationConnector {
    pub fn new() -> Self {
        Self {}
    }

    #[cfg(target_os = "macos")]
    async fn run_osascript_cli(
        &self,
        language: &str,
        script: &str,
    ) -> Result<(String, String, i32), ConnectorError> {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;

        let mut cmd = Command::new("/usr/bin/osascript");
        cmd.arg("-s").arg("s"); // silence: only result text on stdout, errors on stderr
        if matches!(language, "javascript" | "jxa" | "JavaScript" | "JS") {
            cmd.arg("-l").arg("JavaScript");
        }
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(script.as_bytes())
                .await
                .map_err(|e| ConnectorError::Other(e.to_string()))?;
        }
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        let code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok((stdout, stderr, code))
    }

    #[cfg(not(target_os = "macos"))]
    async fn run_osascript_cli(
        &self,
        _language: &str,
        _script: &str,
    ) -> Result<(String, String, i32), ConnectorError> {
        Err(ConnectorError::Other(
            "macOS-only tool called on non-macOS".to_string(),
        ))
    }

    #[cfg(all(target_os = "macos", feature = "macos-automation"))]
    fn try_run_osakit(
        &self,
        language: &str,
        script: &str,
    ) -> Result<Option<String>, ConnectorError> {
        // Best-effort: osakit works reliably when called on the main thread.
        // In multi-threaded runtimes we may still fall back to osascript CLI.
        // We keep this method synchronous and tiny, returning None if unsupported.
        #[cfg(feature = "macos-automation")]
        {
            // Only attempt for AppleScript/JXA source (no external files).
            // SAFETY: osakit APIs are safe abstractions but can panic if not on main thread.
            // We guard with catch_unwind to avoid crashing the server.
            use std::panic::{catch_unwind, AssertUnwindSafe};
            let res = catch_unwind(AssertUnwindSafe(|| -> Result<String, String> {
                // Select language
                let lang = if matches!(language, "javascript" | "jxa" | "JavaScript" | "JS") {
                    osakit::Language::JavaScript
                } else {
                    osakit::Language::AppleScript
                };
                let inst = osakit::Script::new_from_source(lang, script);
                let result = inst
                    .execute()
                    .map_err(|e| format!("osakit runtime error: {}", e))?;
                Ok(result.to_string())
            }));

            match res {
                Ok(Ok(s)) => Ok(Some(s)),
                Ok(Err(_e)) => Ok(None), // fallback to CLI
                Err(_) => Ok(None),      // unwind -> fallback
            }
        }
        #[cfg(not(feature = "macos-automation"))]
        {
            let _ = (language, script);
            Ok(None)
        }
    }

    #[cfg(any(not(target_os = "macos"), not(feature = "macos-automation")))]
    fn try_run_osakit(
        &self,
        _language: &str,
        _script: &str,
    ) -> Result<Option<String>, ConnectorError> {
        Ok(None)
    }

    #[cfg(target_os = "macos")]
    async fn pbpaste(&self) -> Result<String, ConnectorError> {
        use tokio::process::Command;
        let out = Command::new("/usr/bin/pbpaste")
            .output()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    #[cfg(not(target_os = "macos"))]
    async fn pbpaste(&self) -> Result<String, ConnectorError> {
        Err(ConnectorError::Other(
            "Clipboard not available on non-macOS".to_string(),
        ))
    }

    #[cfg(target_os = "macos")]
    async fn pbcopy(&self, input: &str) -> Result<(), ConnectorError> {
        use tokio::io::AsyncWriteExt;
        use tokio::process::Command;
        let mut child = Command::new("/usr/bin/pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .await
                .map_err(|e| ConnectorError::Other(e.to_string()))?;
        }
        let _ = child
            .wait()
            .await
            .map_err(|e| ConnectorError::Other(e.to_string()))?;
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    async fn pbcopy(&self, _input: &str) -> Result<(), ConnectorError> {
        Err(ConnectorError::Other(
            "Clipboard not available on non-macOS".to_string(),
        ))
    }
}

#[async_trait]
impl crate::Connector for MacOsAutomationConnector {
    fn name(&self) -> &'static str {
        "macos"
    }

    fn description(&self) -> &'static str {
        "macOS automation connector providing AppleScript/JXA execution and common helpers (notifications, Finder, clipboard, Shortcuts)."
    }

    fn display_name(&self) -> &'static str {
        "macOS Automation"
    }

    fn icon(&self) -> &'static str {
        "macos"
    }

    fn categories(&self) -> Vec<&'static str> {
        vec!["automation", "productivity", "local"]
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
        // Best-effort: do a no-op script when on macOS to trigger permissions early
        #[cfg(target_os = "macos")]
        {
            let _ = self
                .run_osascript_cli("applescript", "return \"ok\"")
                .await?;
        }
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
                "AppleScript/JXA execution with helper tools. On first use, macOS may prompt for Automation/Accessibility permissions.".to_string(),
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
                name: Cow::Borrowed("run_script"),
                title: Some("Run AppleScript or JXA".to_string()),
                description: Some(Cow::Borrowed(
                    "Execute AppleScript or JXA (requires explicit user permission). Use for \
macOS automation when a dedicated connector doesn't exist. Example: language=\"applescript\" \
script=\"tell application \\\"Finder\\\" to get name of startup disk\" max_output_chars=8000.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "language": {"type": "string", "enum": ["applescript", "javascript", "jxa"], "default": "applescript", "description": "Script language"},
                            "script": {"type": "string", "description": "Script source to execute"},
                            "params": {"description": "Optional parameters exposed as global $params in JXA", "nullable": true},
                            "max_output_chars": {"type": "integer", "minimum": 1, "description": "Optional limit for stdout/stderr length"}
                        },
                        "required": ["script"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: Some(Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "language": {"type": "string"},
                            "stdout": {"type": "string"},
                            "stderr": {"type": "string"},
                            "exit_code": {"type": "integer"},
                            "truncated_stdout": {"type": "boolean"},
                            "truncated_stderr": {"type": "boolean"}
                        },
                        "required": ["language", "stdout", "stderr", "exit_code"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                )),
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("show_notification"),
                title: Some("Show Notification".to_string()),
                description: Some(Cow::Borrowed(
                    "Display a macOS notification. Use for lightweight user alerts. Example: \
message=\"Done\" title=\"rzn-tools\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "title": {"type": "string"},
                            "message": {"type": "string"},
                            "subtitle": {"type": "string"}
                        },
                        "required": ["message"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: Some(Arc::new(json!({
                    "type": "object",
                    "properties": {"ok": {"type":"boolean"}, "exit_code": {"type":"integer"}, "stdout": {"type":"string"}, "stderr": {"type":"string"}},
                    "required": ["ok","exit_code"]
                }).as_object().expect("Schema object").clone())),
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("reveal_in_finder"),
                title: Some("Reveal in Finder".to_string()),
                description: Some(Cow::Borrowed(
                    "Reveal a file/folder in Finder and bring Finder to front. Use when the \
user wants to locate a path visually. Example: path=\"/Users/me/Downloads\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {"path": {"type": "string"}},
                        "required": ["path"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: Some(Arc::new(json!({
                    "type": "object",
                    "properties": {"ok": {"type":"boolean"}, "exit_code": {"type":"integer"}, "stderr": {"type":"string"}},
                    "required": ["ok"]
                }).as_object().expect("Schema object").clone())),
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("get_clipboard"),
                title: Some("Get Clipboard".to_string()),
                description: Some(Cow::Borrowed(
                    "Read the system clipboard as plain text (may contain sensitive data). \
Use only when the user asked you to read it.",
                )),
                input_schema: Arc::new(json!({"type":"object","properties":{}}).as_object().expect("Schema object").clone()),
                output_schema: Some(Arc::new(json!({
                    "type": "object",
                    "properties": {"text": {"type":"string"}},
                    "required": ["text"]
                }).as_object().expect("Schema object").clone())),
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("set_clipboard"),
                title: Some("Set Clipboard".to_string()),
                description: Some(Cow::Borrowed(
                    "Write plain text to the system clipboard. Use when the user wants to \
copy output for manual paste.",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {"text": {"type": "string"}},
                        "required": ["text"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: Some(Arc::new(json!({
                    "type": "object",
                    "properties": {"ok": {"type":"boolean"}},
                    "required": ["ok"]
                }).as_object().expect("Schema object").clone())),
                annotations: None,
                icons: None,
            },
            Tool {
                name: Cow::Borrowed("run_shortcut"),
                title: Some("Run Shortcut".to_string()),
                description: Some(Cow::Borrowed(
                    "Run an Apple Shortcut by name via the shortcuts CLI (requires explicit \
user permission). Use when the user has an existing Shortcut workflow. Example: name=\"Daily Brief\".",
                )),
                input_schema: Arc::new(
                    json!({
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "input": {"type": ["string","object","array","number","boolean","null"], "description": "Optional input; passed as text via stdin"}
                        },
                        "required": ["name"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                output_schema: Some(Arc::new(json!({
                    "type": "object",
                    "properties": {"ok": {"type":"boolean"}, "stdout": {"type":"string"}, "stderr": {"type":"string"}, "exit_code": {"type":"integer"}},
                    "required": ["ok","exit_code"]
                }).as_object().expect("Schema object").clone())),
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
            "run_script" => {
                let language = args
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("applescript");
                let script = args
                    .get("script")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'script'".to_string()))?;
                // Optional params injection for JXA: expose as global $params
                let script_injection =
                    if matches!(language, "javascript" | "jxa" | "JavaScript" | "JS") {
                        if let Some(params) = args.get("params") {
                            let json = serde_json::to_string(params)
                                .map_err(|e| ConnectorError::Other(e.to_string()))?;
                            Some(format!("var $params = {};\n{}", json, script))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                let script_ref: &str = script_injection.as_deref().unwrap_or(script);

                // Prefer osakit if available and usable, else fall back to osascript CLI.
                // Optional truncation
                let max_chars: Option<usize> = args
                    .get("max_output_chars")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);

                if let Ok(Some(mut out)) = self.try_run_osakit(language, script_ref) {
                    let (trunc_stdout, ts) = if let Some(limit) = max_chars {
                        if out.len() > limit {
                            out.truncate(limit);
                            (true, out)
                        } else {
                            (false, out)
                        }
                    } else {
                        (false, out)
                    };
                    let payload = json!({
                        "language": language,
                        "stdout": ts,
                        "stderr": "",
                        "exit_code": 0,
                        "truncated_stdout": trunc_stdout,
                        "truncated_stderr": false
                    });
                    return structured_result_with_text(&payload, None);
                }

                let (mut stdout, mut stderr, code) =
                    self.run_osascript_cli(language, script_ref).await?;
                let (mut trunc_stdout, mut trunc_stderr) = (false, false);
                if let Some(limit) = max_chars {
                    if stdout.len() > limit {
                        stdout.truncate(limit);
                        trunc_stdout = true;
                    }
                    if stderr.len() > limit {
                        stderr.truncate(limit);
                        trunc_stderr = true;
                    }
                }
                let payload = json!({
                    "language": language,
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": code,
                    "truncated_stdout": trunc_stdout,
                    "truncated_stderr": trunc_stderr
                });
                structured_result_with_text(&payload, None)
            }
            "show_notification" => {
                let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("RZN");
                let message = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'message'".to_string())
                    })?;
                let subtitle = args.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
                fn esc(s: &str) -> String {
                    s.replace('"', "\\\"")
                }
                let script = if subtitle.is_empty() {
                    format!(
                        "display notification \"{}\" with title \"{}\"",
                        esc(message),
                        esc(title)
                    )
                } else {
                    format!(
                        "display notification \"{}\" with title \"{}\" subtitle \"{}\"",
                        esc(message),
                        esc(title),
                        esc(subtitle)
                    )
                };
                let (stdout, stderr, code) = self.run_osascript_cli("applescript", &script).await?;
                let payload =
                    json!({"ok": code==0, "stdout": stdout, "stderr": stderr, "exit_code": code});
                structured_result_with_text(&payload, None)
            }
            "reveal_in_finder" => {
                let path = args
                    .get("path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'path'".to_string()))?;
                let script = format!(
                    "tell application \"Finder\" to reveal POSIX file \"{}\"\nactivate application id \"com.apple.finder\"",
                    path.replace('"', "\\\"")
                );
                let (_stdout, stderr, code) =
                    self.run_osascript_cli("applescript", &script).await?;
                let payload = json!({"ok": code==0, "stderr": stderr, "exit_code": code});
                structured_result_with_text(&payload, None)
            }
            "get_clipboard" => {
                let text = self.pbpaste().await?;
                let payload = json!({"text": text});
                structured_result_with_text(&payload, None)
            }
            "set_clipboard" => {
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ConnectorError::InvalidParams("Missing 'text'".to_string()))?;
                self.pbcopy(text).await?;
                let payload = json!({"ok": true});
                structured_result_with_text(&payload, None)
            }
            "run_shortcut" => {
                #[cfg(target_os = "macos")]
                {
                    use tokio::io::AsyncWriteExt;
                    use tokio::process::Command;
                    let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                        ConnectorError::InvalidParams("Missing 'name'".to_string())
                    })?;
                    let input_opt = args.get("input").cloned();
                    let mut cmd = Command::new("/usr/bin/shortcuts");
                    cmd.arg("run").arg(name);
                    // The CLI accepts "-i -" to read stdin; we stringify JSON if provided.
                    let mut child = cmd
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped())
                        .spawn()
                        .map_err(|e| ConnectorError::Other(e.to_string()))?;
                    if let (Some(mut stdin), Some(input)) = (child.stdin.take(), input_opt) {
                        let text = if input.is_string() {
                            input.as_str().unwrap_or("").to_string()
                        } else {
                            serde_json::to_string(&input)
                                .map_err(|e| ConnectorError::Other(e.to_string()))?
                        };
                        stdin
                            .write_all(text.as_bytes())
                            .await
                            .map_err(|e| ConnectorError::Other(e.to_string()))?;
                    }
                    let out = child
                        .wait_with_output()
                        .await
                        .map_err(|e| ConnectorError::Other(e.to_string()))?;
                    let code = out.status.code().unwrap_or(-1);
                    let payload = json!({
                        "ok": code==0,
                        "stdout": String::from_utf8_lossy(&out.stdout),
                        "stderr": String::from_utf8_lossy(&out.stderr),
                        "exit_code": code
                    });
                    return structured_result_with_text(&payload, None);
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return Err(ConnectorError::Other(
                        "Shortcuts CLI not available on non-macOS".to_string(),
                    ));
                }
            }
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
