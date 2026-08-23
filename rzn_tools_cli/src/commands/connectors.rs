use crate::cli::Cli;
use crate::commands::Result;
use crate::output::{format_output, OutputData};
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, ContentArrangement, Table};
use owo_colors::OwoColorize;
use rzn_tools_core::PaginatedRequestParam;
use serde_json::{json, Value};

/// Get the terminal width, defaulting to 80 if detection fails
fn get_terminal_width() -> u16 {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0)
        .unwrap_or(80)
}

/// Truncate text to fit within a given width, adding "..." if truncated
fn truncate_text(text: &str, max_width: usize) -> String {
    if text.len() <= max_width {
        text.to_string()
    } else if max_width > 3 {
        format!("{}...", &text[..max_width - 3])
    } else {
        text.chars().take(max_width).collect()
    }
}

pub async fn run(cli: &Cli) -> Result<()> {
    let registry = crate::commands::list::create_registry(cli.auth_profile.as_deref()).await?;
    let providers = registry.list_providers();

    if providers.is_empty() {
        println!("{}", "No connectors available".yellow());
        return Ok(());
    }

    let mut detailed_info = Vec::new();

    // Gather detailed information about each connector
    for provider_info in &providers {
        if let Some(provider) = registry.get_provider(&provider_info.name) {
            let c = provider.lock().await;
            let mut connector_details = json!({
                "name": provider_info.name,
                "description": provider_info.description,
                "status": "unknown",
                "auth_required": false,
                "tools": [],
                "capabilities": {}
            });

            // Test authentication status
            match c.test_auth().await {
                Ok(_) => {
                    connector_details["status"] = json!("ready");
                }
                Err(_) => {
                    // Mark as needs_auth only if any field is actually required
                    let config_schema = c.config_schema();
                    let requires_any = config_schema.fields.iter().any(|f| f.required);
                    if requires_any {
                        connector_details["status"] = json!("needs_auth");
                        connector_details["auth_required"] = json!(true);
                    } else {
                        // Optional auth: surface as ready to avoid false alarms
                        connector_details["status"] = json!("ready");
                        connector_details["auth_required"] = json!(false);
                    }
                }
            }

            // Get available tools
            if let Ok(tools_response) = c
                .list_tools(Some(PaginatedRequestParam { cursor: None }))
                .await
            {
                let tool_names: Vec<String> = tools_response
                    .tools
                    .iter()
                    .map(|tool| tool.name.to_string())
                    .collect();
                connector_details["tools"] = json!(tool_names);
            }

            connector_details["capabilities"] = json!({
                "tools": connector_details["tools"].as_array().is_some_and(|tools| !tools.is_empty()),
            });

            detailed_info.push(connector_details);
        }
    }

    let output_data = OutputData::ConnectorList(providers.clone());

    match cli.output {
        crate::cli::OutputFormat::Pretty => {
            format_pretty_connectors(&detailed_info)?;
        }
        _ => {
            format_output(&output_data, &cli.output)?;
        }
    }

    Ok(())
}

fn format_pretty_connectors(connectors: &[Value]) -> Result<()> {
    let term_width = get_terminal_width() as usize;

    println!("{}", "Connector Details".bold().cyan());
    println!();

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(term_width as u16)
        .set_header(vec!["Name", "Status", "Tools", "Auth", "Description"]);

    // Calculate max description width
    let desc_width = term_width.saturating_sub(55);

    for connector in connectors {
        let name = connector
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let status = connector
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let status_display = match status {
            "ready" => "✓ Ready",
            "needs_auth" => "⚠ Setup",
            _ => "? Unknown",
        };

        let auth_required = connector
            .get("auth_required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let auth_display = if auth_required { "Required" } else { "None" };

        let tools = connector
            .get("tools")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len().to_string())
            .unwrap_or_else(|| "0".to_string());

        let description = connector
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        table.add_row(vec![
            name.to_string(),
            status_display.to_string(),
            tools,
            auth_display.to_string(),
            truncate_text(description, desc_width.max(30)),
        ]);
    }

    println!("{}", table);
    println!();

    // Show categorized connectors
    print_connector_categories(connectors)?;

    // Show usage tips
    println!("{}", "Usage Tips:".bold().green());
    println!(
        "  {} - List available tools for a connector",
        "rzn-tools tools <connector>".cyan()
    );
    println!(
        "  {} - Configure authentication",
        "rzn-tools config set <connector>".cyan()
    );
    println!(
        "  {} - Test authentication",
        "rzn-tools config test <connector>".cyan()
    );
    println!(
        "  {} - Call a connector tool",
        "rzn-tools call <connector> <tool> --args <JSON_OBJECT>".cyan()
    );

    Ok(())
}

fn print_connector_categories(connectors: &[Value]) -> Result<()> {
    let categories = vec![
        ("🎥 Media & Entertainment", vec!["youtube", "reddit"]),
        (
            "📱 App Stores",
            vec![
                "play-store",
                "app-store",
                "app-store-connect",
                "apple-search-ads",
            ],
        ),
        ("📈 Markets & Forecasting", vec!["polymarket", "kalshi"]),
        (
            "🔍 Search & Discovery",
            vec![
                "google-search-console",
                "bing-webmaster-tools",
                "openai-search",
                "anthropic-search",
                "gemini-search",
                "perplexity-search",
                "xai-search",
                "exa",
                "firecrawl-search",
                "serper-search",
                "tavily-search",
                "serpapi-search",
            ],
        ),
        (
            "📚 Academic & Research",
            vec!["arxiv", "pubmed", "semantic-scholar", "scihub"],
        ),
        (
            "🌐 Web & Social",
            vec!["linkedin", "x", "hackernews", "wikipedia"],
        ),
        ("🛠️ Web Scraping", vec!["web"]),
        (
            "🗂️ Productivity & Cloud",
            vec![
                "caldav",
                "microsoft-graph",
                "google-drive",
                "google-gmail",
                "google-calendar",
                "google-people",
                "imap",
                "smtp",
            ],
        ),
    ];

    for (category, connector_names) in categories {
        let mut found_connectors = Vec::new();

        for connector in connectors {
            if let Some(name) = connector.get("name").and_then(|v| v.as_str()) {
                if connector_names.contains(&name) {
                    let status = connector
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");

                    let status_icon = match status {
                        "ready" => "✓",
                        "needs_auth" => "⚠",
                        _ => "?",
                    };

                    found_connectors.push((name, status_icon));
                }
            }
        }

        if !found_connectors.is_empty() {
            println!("{}", category.bold());
            for (name, status) in found_connectors {
                println!("  {} {}", status, name.cyan());
            }
            println!();
        }
    }

    Ok(())
}

use crate::cli::{YoutubeArgs, YoutubeTools};
use crate::commands::copy_to_clipboard;
use crate::commands::report::render_tool_failure_report_block;
use crate::commands::usage_helpers::print_cost_summary;
use rzn_tools_core::display::from_normalized::{
    stash_original_structured_content_in_meta,
    try_convert_normalized_structured_content_to_display_v1,
};
use rzn_tools_core::CallToolRequestParam;
use serde_json::Map;

async fn call_tool_raw(
    cli: &Cli,
    connector: &str,
    tool: &str,
    args: Map<String, Value>,
) -> Result<(Value, Option<Value>)> {
    let registry = crate::commands::list::create_registry(cli.auth_profile.as_deref()).await?;
    let provider = registry
        .get_provider(connector)
        .ok_or_else(|| crate::commands::CommandError::ConnectorNotFound(connector.to_string()))?;

    let c = provider.lock().await;

    // Validate tool exists and required arguments are present.
    // This prevents the CLI wrappers from silently drifting away from core tool names/schemas.
    let tools_response = c
        .list_tools(Some(PaginatedRequestParam { cursor: None }))
        .await?;
    let tool_def = tools_response
        .tools
        .iter()
        .find(|t| t.name.as_ref() == tool)
        .ok_or_else(|| {
            crate::commands::CommandError::ToolNotFound(tool.to_string(), connector.to_string())
        })?;

    if let Some(required) = tool_def
        .input_schema
        .get("required")
        .and_then(|v| v.as_array())
    {
        let missing: Vec<String> = required
            .iter()
            .filter_map(|v| v.as_str())
            .filter(|k| !args.contains_key(*k))
            .map(ToString::to_string)
            .collect();
        if !missing.is_empty() {
            return Err(crate::commands::CommandError::InvalidInput(format!(
                "Missing required args for {}.{}: {}",
                connector,
                tool,
                missing.join(", ")
            )));
        }
    }

    let mut args = args;
    let requested_display_v1 = args
        .get("output_format")
        .and_then(|v| v.as_str())
        .is_some_and(|v| v == "display_v1");
    if requested_display_v1 {
        args.insert(
            "output_format".to_string(),
            Value::String("normalized_v1".to_string()),
        );
    }

    let request = CallToolRequestParam {
        name: tool.to_string().into(),
        arguments: Some(args),
    };

    let mut result = match c.call_tool(request).await {
        Ok(result) => result,
        Err(err) => {
            let error = err.to_string();
            eprintln!();
            eprintln!(
                "{}",
                render_tool_failure_report_block(connector, tool, &error)
            );
            eprintln!();
            return Err(err.into());
        }
    };

    if requested_display_v1 && !result.is_error.unwrap_or(false) {
        if let Some(structured) = result.structured_content.as_ref() {
            if let Some(converted) =
                try_convert_normalized_structured_content_to_display_v1(structured)?
            {
                stash_original_structured_content_in_meta(
                    &mut result.meta,
                    structured,
                    "normalized_v1",
                );
                result.structured_content = Some(converted);
            }
        }
    }

    let meta_value = result
        .meta
        .as_ref()
        .and_then(|m| serde_json::to_value(m).ok());

    let payload = if let Some(sc) = result.structured_content {
        sc
    } else {
        serde_json::to_value(&result).unwrap_or_else(|_| json!({"ok": true}))
    };

    Ok((payload, meta_value))
}

fn parse_call_args(raw: &str) -> Result<Map<String, Value>> {
    let value: Value = serde_json::from_str(raw).map_err(|err| {
        crate::commands::CommandError::InvalidInput(format!("Invalid JSON for --args: {err}"))
    })?;
    value.as_object().cloned().ok_or_else(|| {
        crate::commands::CommandError::InvalidInput(
            "--args must be a JSON object; inspect `rzn-tools tools <connector>` for its shape"
                .to_string(),
        )
    })
}

fn output_tool_result(
    cli: &Cli,
    connector: &str,
    tool: &str,
    payload: &Value,
    meta_value: Option<&Value>,
) -> Result<()> {
    match cli.output {
        crate::cli::OutputFormat::Pretty => {
            println!(
                "{} {}.{}",
                "Tool".bold().cyan(),
                connector.yellow(),
                tool.cyan()
            );
            println!();
            println!("{}", crate::output::format_pretty(payload));
        }
        _ => {
            let data = OutputData::CallResult {
                connector: connector.to_string(),
                tool: tool.to_string(),
                result: payload.clone(),
                meta: meta_value.cloned(),
            };
            format_output(&data, &cli.output)?;
        }
    }

    if cli.copy {
        let text = serde_json::to_string_pretty(payload)?;
        copy_to_clipboard(&text)?;
    }

    print_cost_summary(&cli.output, meta_value);

    Ok(())
}

/// Helper to call a connector tool with JSON args
async fn call_tool(cli: &Cli, connector: &str, tool: &str, args: Map<String, Value>) -> Result<()> {
    let (payload, meta_value) = call_tool_raw(cli, connector, tool, args).await?;
    output_tool_result(cli, connector, tool, &payload, meta_value.as_ref())
}

/// Call a discovered connector tool without maintaining a second argument schema in the CLI.
pub async fn call(cli: &Cli, connector: &str, tool: &str, raw_args: &str) -> Result<()> {
    call_tool(cli, connector, tool, parse_call_args(raw_args)?).await
}

/// Handle youtube commands
pub async fn handle_youtube(cli: &Cli, args: YoutubeArgs) -> Result<()> {
    let tool = match args.command {
        Some(t) => t,
        None => YoutubeTools::Get {
            id_or_url: args.id_or_url,
            id: None,
        },
    };

    match tool {
        YoutubeTools::Search { query, limit } => {
            let mut tool_args = Map::new();
            tool_args.insert("query".to_string(), json!(query));
            tool_args.insert("limit".to_string(), json!(limit));
            call_tool(cli, "youtube", "search", tool_args).await
        }
        YoutubeTools::List {
            channel,
            playlist,
            limit,
            within_days,
            published_after,
        } => {
            let mut tool_args = Map::new();
            tool_args.insert(
                "source".to_string(),
                json!(if channel.is_some() {
                    "channel"
                } else {
                    "playlist"
                }),
            );
            if let Some(ch) = channel {
                tool_args.insert("channel".to_string(), json!(ch));
            }
            if let Some(pl) = playlist {
                tool_args.insert("playlist".to_string(), json!(pl));
            }
            if let Some(limit) = limit {
                tool_args.insert("limit".to_string(), json!(limit));
            }
            if let Some(d) = within_days {
                tool_args.insert("published_within_days".to_string(), json!(d));
            }
            if let Some(pa) = published_after {
                tool_args.insert("published_after".to_string(), json!(pa));
            }
            call_tool(cli, "youtube", "list", tool_args).await
        }
        YoutubeTools::ResolveChannel {
            query,
            channel,
            limit,
            prefer_verified,
        } => {
            let mut tool_args = Map::new();
            if let Some(q) = query {
                tool_args.insert("query".to_string(), json!(q));
            }
            if let Some(ch) = channel {
                tool_args.insert("channel".to_string(), json!(ch));
            }
            tool_args.insert("limit".to_string(), json!(limit));
            tool_args.insert("prefer_verified".to_string(), json!(prefer_verified));
            call_tool(cli, "youtube", "resolve_channel", tool_args).await
        }
        YoutubeTools::Get { id_or_url, id } => {
            let id = id_or_url.or(id).ok_or_else(|| {
                crate::commands::CommandError::InvalidInput(
                    "Missing YouTube ID/URL. Provide `rzn-tools youtube <ID_OR_URL>` or `rzn-tools youtube get --id <ID_OR_URL>`.".to_string(),
                )
            })?;

            let mut tool_args = Map::new();
            tool_args.insert("video_id".to_string(), json!(id));
            tool_args.insert("response_format".to_string(), json!("detailed"));
            call_tool(cli, "youtube", "get", tool_args).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_call_args;

    #[test]
    fn call_args_require_a_json_object() {
        assert_eq!(
            parse_call_args(r#"{"query":"rust","limit":3}"#).unwrap()["limit"],
            3
        );
        assert!(parse_call_args("[]").is_err());
        assert!(parse_call_args("not-json").is_err());
    }
}
