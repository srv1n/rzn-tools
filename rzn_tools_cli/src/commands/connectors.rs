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

            // Get capabilities
            let capabilities = c.capabilities().await;
            connector_details["capabilities"] = json!({
                "tools": capabilities.tools.is_some(),
                "resources": capabilities.resources.is_some(),
                "prompts": capabilities.prompts.is_some(),
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
        "  {} - Search using a connector",
        "rzn-tools search <connector> <query>".cyan()
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
                "exa-search",
                "firecrawl-search",
                "serper-search",
                "tavily-search",
                "serpapi-search",
            ],
        ),
        (
            "📚 Academic & Research",
            vec!["arxiv", "pubmed", "semantic_scholar", "scihub"],
        ),
        (
            "🌐 Web & Social",
            vec!["linkedin", "x", "hackernews", "wikipedia"],
        ),
        ("🛠️ Web Scraping", vec!["web", "web_chrome"]),
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

// ============================================================================
// Connector-specific command handlers with proper CLI flags
// ============================================================================

use crate::cli::{
    AnthropicSearchTools, AppStoreConnectTools, AppStoreTools, AppleMessagesTools,
    AppleSearchAdsTools, ArxivTools, AtlassianTools, BingWebmasterToolsTools, BiorxivTools,
    CaldavTools, DiscordTools, ExaTools, FirecrawlSearchTools, GeminiSearchTools, GithubTools,
    GoogleCalendarTools, GoogleDriveTools, GoogleGmailTools, GooglePeopleTools, GoogleScholarTools,
    GoogleSearchConsoleTools, HackernewsTools, ImapTools, KalshiTools, LinkedinTools, LocalfsTools,
    MacosTools, MicrosoftGraphTools, OpenaiSearchTools, ParallelSearchTools, PerplexitySearchTools,
    PlayStoreTools, PolymarketTools, PubmedTools, RedditTools, RssTools, ScihubTools,
    SemanticScholarTools, SerpapiSearchTools, SerperSearchTools, SlackTools, SmtpTools,
    SpotlightTools, TavilySearchTools, WebTools, WikipediaTools, XApiTools, XTools, XaiSearchTools,
    YoutubeArgs, YoutubeTools,
};
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

fn parse_json_argument(name: &str, raw: &str) -> Result<Value> {
    serde_json::from_str(raw).map_err(|err| {
        crate::commands::CommandError::InvalidInput(format!("Invalid JSON for {name}: {err}"))
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

/// Handle OpenAI Search commands
pub async fn handle_openai_search(cli: &Cli, tool: OpenaiSearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        OpenaiSearchTools::Search {
            query,
            limit,
            model,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            if let Some(m) = model {
                args.insert("model".to_string(), json!(m));
            }
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "openai-search", tool_name, args).await
}

/// Handle Anthropic Search commands
pub async fn handle_anthropic_search(cli: &Cli, tool: AnthropicSearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        AnthropicSearchTools::Search {
            query,
            limit,
            model,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            if let Some(m) = model {
                args.insert("model".to_string(), json!(m));
            }
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "anthropic-search", tool_name, args).await
}

/// Handle Gemini Search commands
pub async fn handle_gemini_search(cli: &Cli, tool: GeminiSearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        GeminiSearchTools::Search {
            query,
            limit,
            model,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            if let Some(m) = model {
                args.insert("model".to_string(), json!(m));
            }
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "gemini-search", tool_name, args).await
}

/// Handle Perplexity Search commands
pub async fn handle_perplexity_search(cli: &Cli, tool: PerplexitySearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        PerplexitySearchTools::Search {
            query,
            limit,
            model,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            if let Some(m) = model {
                args.insert("model".to_string(), json!(m));
            }
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "perplexity-search", tool_name, args).await
}

/// Handle xAI Search commands
pub async fn handle_xai_search(cli: &Cli, tool: XaiSearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        XaiSearchTools::Search {
            query,
            limit,
            model,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            if let Some(m) = model {
                args.insert("model".to_string(), json!(m));
            }
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "xai-search", tool_name, args).await
}

/// Handle Exa commands
pub async fn handle_exa(cli: &Cli, tool: ExaTools) -> Result<()> {
    let (tool_name, args) = match tool {
        ExaTools::Search {
            query,
            limit,
            type_,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            args.insert("type".to_string(), json!(type_));
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
        ExaTools::GetContents { ids } => {
            let mut args = Map::new();
            let ids_array: Vec<String> = ids.split(',').map(|s| s.trim().to_string()).collect();
            args.insert("ids".to_string(), json!(ids_array));
            ("get_contents", args)
        }
        ExaTools::FindSimilar { url, limit } => {
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            args.insert("limit".to_string(), json!(limit));
            ("find_similar", args)
        }
        ExaTools::Answer { query, mode } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            if let Some(m) = mode {
                args.insert("mode".to_string(), json!(m));
            }
            ("answer", args)
        }
    };

    call_tool(cli, "exa", tool_name, args).await
}

/// Handle Tavily Search commands
pub async fn handle_tavily_search(cli: &Cli, tool: TavilySearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        TavilySearchTools::Search {
            query,
            limit,
            depth,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            args.insert("depth".to_string(), json!(depth));
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "tavily-search", tool_name, args).await
}

/// Handle Serper Search commands
pub async fn handle_serper_search(cli: &Cli, tool: SerperSearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        SerperSearchTools::Search {
            query,
            limit,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "serper-search", tool_name, args).await
}

/// Handle SerpAPI Search commands
pub async fn handle_serpapi_search(cli: &Cli, tool: SerpapiSearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        SerpapiSearchTools::Search {
            query,
            limit,
            engine,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            args.insert("engine".to_string(), json!(engine));
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "serpapi-search", tool_name, args).await
}

/// Handle Firecrawl Search commands
pub async fn handle_firecrawl_search(cli: &Cli, tool: FirecrawlSearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        FirecrawlSearchTools::Search {
            query,
            limit,
            scrape,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            args.insert("scrape".to_string(), json!(scrape));
            args.insert("response_format".to_string(), json!(response_format));
            ("search", args)
        }
    };

    call_tool(cli, "firecrawl-search", tool_name, args).await
}

/// Handle Parallel Search commands
pub async fn handle_parallel_search(cli: &Cli, tool: ParallelSearchTools) -> Result<()> {
    let (tool_name, args) = match tool {
        ParallelSearchTools::Search { query, limit } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
    };

    call_tool(cli, "parallel-search", tool_name, args).await
}

/// Handle CalDAV commands
pub async fn handle_caldav(cli: &Cli, tool: CaldavTools) -> Result<()> {
    let (tool_name, args) = match tool {
        CaldavTools::ListCalendars { response_format } => {
            let mut args = Map::new();
            args.insert("response_format".to_string(), json!(response_format));
            ("list_calendars", args)
        }
        CaldavTools::ListEvents {
            calendar_url,
            limit,
            cursor,
            time_min,
            time_max,
            output_format,
            response_format,
        } => {
            let mut args = Map::new();
            if let Some(url) = calendar_url {
                args.insert("calendar_url".to_string(), json!(url));
            }
            args.insert("limit".to_string(), json!(limit));
            if let Some(cursor_value) = cursor {
                args.insert("cursor".to_string(), json!(cursor_value));
            }
            if let Some(start) = time_min {
                args.insert("time_min".to_string(), json!(start));
            }
            if let Some(end) = time_max {
                args.insert("time_max".to_string(), json!(end));
            }
            args.insert("output_format".to_string(), json!(output_format));
            args.insert("response_format".to_string(), json!(response_format));
            ("list", args)
        }
        CaldavTools::GetEvent {
            item_ref,
            url,
            event_url,
            output_format,
            response_format,
        } => {
            let mut args = Map::new();
            if let Some(reference) = item_ref {
                args.insert("item_ref".to_string(), json!(reference));
            }
            if let Some(url_value) = url {
                args.insert("url".to_string(), json!(url_value));
            }
            if let Some(event_url_value) = event_url {
                args.insert("event_url".to_string(), json!(event_url_value));
            }
            args.insert("output_format".to_string(), json!(output_format));
            args.insert("response_format".to_string(), json!(response_format));
            ("get", args)
        }
        CaldavTools::CreateEvent {
            calendar_url,
            event_path,
            url,
            event_url,
            uid,
            summary,
            description,
            location,
            status,
            organizer,
            start,
            end,
            raw_ical,
            output_format,
            response_format,
        } => {
            let mut args = Map::new();
            if let Some(value) = calendar_url {
                args.insert("calendar_url".to_string(), json!(value));
            }
            if let Some(value) = event_path {
                args.insert("event_path".to_string(), json!(value));
            }
            if let Some(value) = url {
                args.insert("url".to_string(), json!(value));
            }
            if let Some(value) = event_url {
                args.insert("event_url".to_string(), json!(value));
            }
            if let Some(value) = uid {
                args.insert("uid".to_string(), json!(value));
            }
            if let Some(value) = summary {
                args.insert("summary".to_string(), json!(value));
            }
            if let Some(value) = description {
                args.insert("description".to_string(), json!(value));
            }
            if let Some(value) = location {
                args.insert("location".to_string(), json!(value));
            }
            if let Some(value) = status {
                args.insert("status".to_string(), json!(value));
            }
            if let Some(value) = organizer {
                args.insert("organizer".to_string(), json!(value));
            }
            if let Some(value) = start {
                args.insert("start".to_string(), json!(value));
            }
            if let Some(value) = end {
                args.insert("end".to_string(), json!(value));
            }
            if let Some(value) = raw_ical {
                args.insert("raw_ical".to_string(), json!(value));
            }
            args.insert("output_format".to_string(), json!(output_format));
            args.insert("response_format".to_string(), json!(response_format));
            ("create", args)
        }
        CaldavTools::UpdateEvent {
            item_ref,
            url,
            event_url,
            if_match,
            uid,
            summary,
            description,
            location,
            status,
            organizer,
            start,
            end,
            raw_ical,
            output_format,
            response_format,
        } => {
            let mut args = Map::new();
            if let Some(value) = item_ref {
                args.insert("item_ref".to_string(), json!(value));
            }
            if let Some(value) = url {
                args.insert("url".to_string(), json!(value));
            }
            if let Some(value) = event_url {
                args.insert("event_url".to_string(), json!(value));
            }
            if let Some(value) = if_match {
                args.insert("if_match".to_string(), json!(value));
            }
            if let Some(value) = uid {
                args.insert("uid".to_string(), json!(value));
            }
            if let Some(value) = summary {
                args.insert("summary".to_string(), json!(value));
            }
            if let Some(value) = description {
                args.insert("description".to_string(), json!(value));
            }
            if let Some(value) = location {
                args.insert("location".to_string(), json!(value));
            }
            if let Some(value) = status {
                args.insert("status".to_string(), json!(value));
            }
            if let Some(value) = organizer {
                args.insert("organizer".to_string(), json!(value));
            }
            if let Some(value) = start {
                args.insert("start".to_string(), json!(value));
            }
            if let Some(value) = end {
                args.insert("end".to_string(), json!(value));
            }
            if let Some(value) = raw_ical {
                args.insert("raw_ical".to_string(), json!(value));
            }
            args.insert("output_format".to_string(), json!(output_format));
            args.insert("response_format".to_string(), json!(response_format));
            ("update", args)
        }
        CaldavTools::DeleteEvent {
            item_ref,
            url,
            event_url,
            if_match,
        } => {
            let mut args = Map::new();
            if let Some(value) = item_ref {
                args.insert("item_ref".to_string(), json!(value));
            }
            if let Some(value) = url {
                args.insert("url".to_string(), json!(value));
            }
            if let Some(value) = event_url {
                args.insert("event_url".to_string(), json!(value));
            }
            if let Some(value) = if_match {
                args.insert("if_match".to_string(), json!(value));
            }
            ("delete", args)
        }
    };

    call_tool(cli, "caldav", tool_name, args).await
}

/// Handle Google Calendar commands
pub async fn handle_google_calendar(cli: &Cli, tool: GoogleCalendarTools) -> Result<()> {
    let (tool_name, args) = match tool {
        GoogleCalendarTools::ListEvents {
            max_results,
            page_token,
            time_min,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("max_results".to_string(), json!(max_results));
            if let Some(t) = page_token {
                args.insert("page_token".to_string(), json!(t));
            }
            if let Some(time) = time_min {
                args.insert("time_min".to_string(), json!(time));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("list_events", args)
        }
        GoogleCalendarTools::CreateEvent {
            summary,
            start,
            end,
        } => {
            let mut args = Map::new();
            args.insert("summary".to_string(), json!(summary));
            args.insert("start".to_string(), json!(start));
            args.insert("end".to_string(), json!(end));
            ("create_event", args)
        }
        GoogleCalendarTools::SyncEvents {
            sync_token,
            max_results,
        } => {
            let mut args = Map::new();
            args.insert("sync_token".to_string(), json!(sync_token));
            args.insert("max_results".to_string(), json!(max_results));
            ("sync_events", args)
        }
        GoogleCalendarTools::UpdateEvent {
            event_id,
            summary,
            start,
            end,
        } => {
            let mut args = Map::new();
            args.insert("event_id".to_string(), json!(event_id));
            if let Some(s) = summary {
                args.insert("summary".to_string(), json!(s));
            }
            if let Some(st) = start {
                args.insert("start".to_string(), json!(st));
            }
            if let Some(e) = end {
                args.insert("end".to_string(), json!(e));
            }
            ("update_event", args)
        }
        GoogleCalendarTools::DeleteEvent { event_id } => {
            let mut args = Map::new();
            if let Some(id) = event_id {
                args.insert("event_id".to_string(), json!(id));
            }
            ("delete_event", args)
        }
    };

    call_tool(cli, "google-calendar", tool_name, args).await
}

/// Handle Google Drive commands
pub async fn handle_google_drive(cli: &Cli, tool: GoogleDriveTools) -> Result<()> {
    let (tool_name, args) = match tool {
        GoogleDriveTools::ListFiles {
            q,
            page_size,
            limit,
            page_token,
            response_format,
        } => {
            let mut args = Map::new();
            if let Some(query) = q {
                args.insert("q".to_string(), json!(query));
            }
            args.insert("page_size".to_string(), json!(page_size));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            if let Some(t) = page_token {
                args.insert("page_token".to_string(), json!(t));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("list_files", args)
        }
        GoogleDriveTools::GetFile {
            file_id,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("file_id".to_string(), json!(file_id));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_file", args)
        }
        GoogleDriveTools::DownloadFile { file_id, max_bytes } => {
            let mut args = Map::new();
            args.insert("file_id".to_string(), json!(file_id));
            if let Some(mb) = max_bytes {
                args.insert("max_bytes".to_string(), json!(mb));
            }
            ("download_file", args)
        }
        GoogleDriveTools::ExportFile { file_id, mime_type } => {
            let mut args = Map::new();
            args.insert("file_id".to_string(), json!(file_id));
            args.insert("mime_type".to_string(), json!(mime_type));
            ("export_file", args)
        }
        GoogleDriveTools::UploadFile {
            name,
            mime_type,
            data_base64,
            parents,
        } => {
            let mut args = Map::new();
            args.insert("name".to_string(), json!(name));
            args.insert("mime_type".to_string(), json!(mime_type));
            args.insert("data_base64".to_string(), json!(data_base64));
            if let Some(p) = parents {
                let parents_vec: Vec<String> = p.split(',').map(|s| s.trim().to_string()).collect();
                args.insert("parents".to_string(), json!(parents_vec));
            }
            ("upload_file", args)
        }
        GoogleDriveTools::UploadFileResumable {
            name,
            mime_type,
            data_base64,
            parents,
        } => {
            let mut args = Map::new();
            args.insert("name".to_string(), json!(name));
            args.insert("mime_type".to_string(), json!(mime_type));
            args.insert("data_base64".to_string(), json!(data_base64));
            if let Some(p) = parents {
                let parents_vec: Vec<String> = p.split(',').map(|s| s.trim().to_string()).collect();
                args.insert("parents".to_string(), json!(parents_vec));
            }
            ("upload_file_resumable", args)
        }
    };

    call_tool(cli, "google-drive", tool_name, args).await
}

/// Handle Google Gmail commands
pub async fn handle_google_gmail(cli: &Cli, tool: GoogleGmailTools) -> Result<()> {
    let (tool_name, args) = match tool {
        GoogleGmailTools::ListMessages {
            q,
            max_results,
            page_token,
            response_format,
        } => {
            let mut args = Map::new();
            if let Some(query) = q {
                args.insert("q".to_string(), json!(query));
            }
            args.insert("max_results".to_string(), json!(max_results));
            if let Some(t) = page_token {
                args.insert("page_token".to_string(), json!(t));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("list_messages", args)
        }
        GoogleGmailTools::DecodeMessageRaw { raw_base64url } => {
            let mut args = Map::new();
            args.insert("raw_base64url".to_string(), json!(raw_base64url));
            ("decode_message_raw", args)
        }
        GoogleGmailTools::GetMessage {
            id,
            format,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("id".to_string(), json!(id));
            args.insert("format".to_string(), json!(format));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_message", args)
        }
        GoogleGmailTools::GetThread { id } => {
            let mut args = Map::new();
            args.insert("id".to_string(), json!(id));
            ("get_thread", args)
        }
    };

    call_tool(cli, "google-gmail", tool_name, args).await
}

/// Handle Google People commands
pub async fn handle_google_people(cli: &Cli, tool: GooglePeopleTools) -> Result<()> {
    let (tool_name, args) = match tool {
        GooglePeopleTools::ListConnections {
            page_size,
            limit,
            page_token,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("page_size".to_string(), json!(page_size));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            if let Some(t) = page_token {
                args.insert("page_token".to_string(), json!(t));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("list_connections", args)
        }
        GooglePeopleTools::GetPerson {
            resource_name,
            person_fields,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("resource_name".to_string(), json!(resource_name));
            if let Some(fields) = person_fields {
                args.insert("person_fields".to_string(), json!(fields));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_person", args)
        }
    };

    call_tool(cli, "google-people", tool_name, args).await
}

/// Handle Google Search Console commands
pub async fn handle_google_search_console(cli: &Cli, tool: GoogleSearchConsoleTools) -> Result<()> {
    let (tool_name, args) = match tool {
        GoogleSearchConsoleTools::ListSites { response_format } => {
            let mut args = Map::new();
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("list_sites", args)
        }
        GoogleSearchConsoleTools::GetSite {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_site", args)
        }
        GoogleSearchConsoleTools::SearchAnalytics {
            site_url,
            start_date,
            end_date,
            dimensions,
            row_limit,
            start_row,
            aggregation_type,
            r#type,
            data_state,
            dimension_filter_groups,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("start_date".to_string(), json!(start_date));
            args.insert("end_date".to_string(), json!(end_date));
            if let Some(d) = dimensions {
                args.insert("dimensions".to_string(), json!(d));
            }
            args.insert("row_limit".to_string(), json!(row_limit));
            args.insert("start_row".to_string(), json!(start_row));
            args.insert("aggregation_type".to_string(), json!(aggregation_type));
            if let Some(t) = r#type {
                args.insert("type".to_string(), json!(t));
            }
            args.insert("data_state".to_string(), json!(data_state));
            if let Some(f) = dimension_filter_groups {
                args.insert("dimension_filter_groups".to_string(), json!(f));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("search_analytics", args)
        }
        GoogleSearchConsoleTools::ListSitemaps {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("list_sitemaps", args)
        }
        GoogleSearchConsoleTools::GetSitemap {
            site_url,
            feedpath,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("feedpath".to_string(), json!(feedpath));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_sitemap", args)
        }
        GoogleSearchConsoleTools::SubmitSitemap {
            site_url,
            feedpath,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("feedpath".to_string(), json!(feedpath));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("submit_sitemap", args)
        }
        GoogleSearchConsoleTools::DeleteSitemap {
            site_url,
            feedpath,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("feedpath".to_string(), json!(feedpath));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("delete_sitemap", args)
        }
        GoogleSearchConsoleTools::InspectUrl {
            site_url,
            inspection_url,
            language_code,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("inspection_url".to_string(), json!(inspection_url));
            if let Some(lang) = language_code {
                args.insert("language_code".to_string(), json!(lang));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("inspect_url", args)
        }
        GoogleSearchConsoleTools::QueryBuilder {
            query_type,
            site_url,
            days,
            filter,
        } => {
            let mut args = Map::new();
            args.insert("query_type".to_string(), json!(query_type));
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("days".to_string(), json!(days));
            if let Some(f) = filter {
                args.insert("filter".to_string(), json!(f));
            }
            ("query_builder", args)
        }
    };

    call_tool(cli, "google-search-console", tool_name, args).await
}

/// Handle Bing Webmaster Tools commands
pub async fn handle_bing_webmaster_tools(cli: &Cli, tool: BingWebmasterToolsTools) -> Result<()> {
    let (tool_name, args) = match tool {
        BingWebmasterToolsTools::ListSites { response_format } => {
            let mut args = Map::new();
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("list_sites", args)
        }
        BingWebmasterToolsTools::GetRankAndTrafficStats {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_rank_and_traffic_stats", args)
        }
        BingWebmasterToolsTools::GetCrawlStats {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_crawl_stats", args)
        }
        BingWebmasterToolsTools::GetCrawlIssues {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_crawl_issues", args)
        }
        BingWebmasterToolsTools::GetKeywordData {
            query,
            country,
            language,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("country".to_string(), json!(country));
            args.insert("language".to_string(), json!(language));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_keyword_data", args)
        }
        BingWebmasterToolsTools::GetBacklinks {
            site_url,
            page,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("page".to_string(), json!(page));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_backlinks", args)
        }
        BingWebmasterToolsTools::GetQueryStats {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_query_stats", args)
        }
        BingWebmasterToolsTools::GetQueryTrafficStats {
            site_url,
            query,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("query".to_string(), json!(query));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_query_traffic_stats", args)
        }
        BingWebmasterToolsTools::GetPageStats {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_page_stats", args)
        }
        BingWebmasterToolsTools::GetUrlSubmissionQuota {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_url_submission_quota", args)
        }
        BingWebmasterToolsTools::SubmitUrl { site_url, url } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("url".to_string(), json!(url));
            ("submit_url", args)
        }
        BingWebmasterToolsTools::SubmitUrlBatch { site_url, url_list } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let urls: Vec<String> = url_list
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            args.insert("url_list".to_string(), json!(urls));
            ("submit_url_batch", args)
        }
        BingWebmasterToolsTools::IndexNowSubmitUrl {
            url,
            host,
            key_location,
        } => {
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            if let Some(h) = host {
                args.insert("host".to_string(), json!(h));
            }
            if let Some(kl) = key_location {
                args.insert("key_location".to_string(), json!(kl));
            }
            ("indexnow_submit_url", args)
        }
        BingWebmasterToolsTools::IndexNowSubmitUrlBatch {
            url_list,
            host,
            key_location,
        } => {
            let mut args = Map::new();
            let urls: Vec<String> = url_list
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            args.insert("url_list".to_string(), json!(urls));
            if let Some(h) = host {
                args.insert("host".to_string(), json!(h));
            }
            if let Some(kl) = key_location {
                args.insert("key_location".to_string(), json!(kl));
            }
            ("indexnow_submit_url_batch", args)
        }
        BingWebmasterToolsTools::GetUrlInfo {
            site_url,
            url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            args.insert("url".to_string(), json!(url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_url_info", args)
        }
        BingWebmasterToolsTools::GetDeepLinks {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_deep_links", args)
        }
        BingWebmasterToolsTools::GetBlockedUrls {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_blocked_urls", args)
        }
        BingWebmasterToolsTools::GetQueryPageStats {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_query_page_stats", args)
        }
        BingWebmasterToolsTools::AddSite {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("add_site", args)
        }
        BingWebmasterToolsTools::VerifySite {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("verify_site", args)
        }
        BingWebmasterToolsTools::GetContentIssues {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_content_issues", args)
        }
        BingWebmasterToolsTools::GetMalwareIssues {
            site_url,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("site_url".to_string(), json!(site_url));
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            args.insert("response_format".to_string(), json!(response_format));
            ("get_malware_issues", args)
        }
    };

    call_tool(cli, "bing-webmaster-tools", tool_name, args).await
}

pub async fn handle_linkedin(cli: &Cli, tool: LinkedinTools) -> Result<()> {
    let (tool_name, args) = match tool {
        LinkedinTools::AuthStatus => ("get_auth_status", Map::new()),
        LinkedinTools::Me => ("get_me", Map::new()),
        LinkedinTools::Share {
            text,
            visibility,
            url,
            image,
            title,
            description,
            author,
        } => {
            let mut args = Map::new();
            args.insert("text".to_string(), json!(text));
            args.insert("visibility".to_string(), json!(visibility));
            if let Some(url) = url {
                args.insert("url".to_string(), json!(url));
            }
            if let Some(image) = image {
                args.insert("image".to_string(), json!(image));
            }
            if let Some(title) = title {
                args.insert("title".to_string(), json!(title));
            }
            if let Some(description) = description {
                args.insert("description".to_string(), json!(description));
            }
            if let Some(author) = author {
                args.insert("author".to_string(), json!(author));
            }
            ("create_share_update", args)
        }
        LinkedinTools::CompanyShare {
            organization,
            text,
            visibility,
            url,
            image,
            title,
            description,
        } => {
            let mut args = Map::new();
            args.insert("text".to_string(), json!(text));
            args.insert("visibility".to_string(), json!(visibility));
            if let Some(organization) = organization {
                args.insert("organization".to_string(), json!(organization));
            }
            if let Some(url) = url {
                args.insert("url".to_string(), json!(url));
            }
            if let Some(image) = image {
                args.insert("image".to_string(), json!(image));
            }
            if let Some(title) = title {
                args.insert("title".to_string(), json!(title));
            }
            if let Some(description) = description {
                args.insert("description".to_string(), json!(description));
            }
            ("create_company_update", args)
        }
        LinkedinTools::ApiRequest {
            method,
            path,
            query_json,
            headers_json,
            body_json,
            body,
            linkedin_version,
            include_linkedin_rest_headers,
        } => {
            let mut args = Map::new();
            args.insert("method".to_string(), json!(method));
            args.insert("path".to_string(), json!(path));
            if let Some(query_json) = query_json {
                args.insert(
                    "query".to_string(),
                    parse_json_argument("query_json", &query_json)?,
                );
            }
            if let Some(headers_json) = headers_json {
                args.insert(
                    "headers".to_string(),
                    parse_json_argument("headers_json", &headers_json)?,
                );
            }
            if let Some(body_json) = body_json {
                args.insert(
                    "body".to_string(),
                    parse_json_argument("body_json", &body_json)?,
                );
            } else if let Some(body) = body {
                args.insert("body".to_string(), json!(body));
            }
            if let Some(linkedin_version) = linkedin_version {
                args.insert("linkedin_version".to_string(), json!(linkedin_version));
            }
            if include_linkedin_rest_headers {
                args.insert(
                    "include_linkedin_rest_headers".to_string(),
                    json!(include_linkedin_rest_headers),
                );
            }
            ("api_request", args)
        }
        LinkedinTools::RefreshToken => ("refresh_access_token", Map::new()),
    };

    call_tool(cli, "linkedin", tool_name, args).await
}

/// Handle Google Scholar commands
pub async fn handle_google_scholar(cli: &Cli, tool: GoogleScholarTools) -> Result<()> {
    let (tool_name, args) = match tool {
        GoogleScholarTools::SearchPapers { query, limit } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            ("search_papers", args)
        }
    };

    call_tool(cli, "google-scholar", tool_name, args).await
}

/// Handle Atlassian commands
pub async fn handle_atlassian(cli: &Cli, tool: AtlassianTools) -> Result<()> {
    let (tool_name, args) = match tool {
        AtlassianTools::TestAuth => ("test_auth", Map::new()),
        AtlassianTools::JiraSearch {
            jql,
            start_at,
            max_results,
            fields,
        } => {
            let mut args = Map::new();
            args.insert("jql".to_string(), json!(jql));
            if start_at > 0 {
                args.insert("start_at".to_string(), json!(start_at));
            }
            if max_results != 50 {
                args.insert("max_results".to_string(), json!(max_results));
            }
            if let Some(f) = fields {
                args.insert("fields".to_string(), json!(f));
            }
            ("jira_search_issues", args)
        }
        AtlassianTools::JiraGet { key, expand } => {
            let mut args = Map::new();
            args.insert("key".to_string(), json!(key));
            if let Some(e) = expand {
                args.insert("expand".to_string(), json!(e));
            }
            ("jira_get_issue", args)
        }
        AtlassianTools::ConfSearch { cql, start, limit } => {
            let mut args = Map::new();
            args.insert("cql".to_string(), json!(cql));
            if start > 0 {
                args.insert("start".to_string(), json!(start));
            }
            if limit != 25 {
                args.insert("limit".to_string(), json!(limit));
            }
            ("conf_search_pages", args)
        }
        AtlassianTools::ConfGet { id, expand } => {
            let mut args = Map::new();
            args.insert("id".to_string(), json!(id));
            if let Some(e) = expand {
                args.insert("expand".to_string(), json!(e));
            }
            ("conf_get_page", args)
        }
    };

    call_tool(cli, "atlassian", tool_name, args).await
}

/// Handle Microsoft Graph commands
pub async fn handle_microsoft_graph(cli: &Cli, tool: MicrosoftGraphTools) -> Result<()> {
    let (tool_name, args) = match tool {
        MicrosoftGraphTools::ListMessages {
            top,
            next_link,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("top".to_string(), json!(top));
            if let Some(nl) = next_link {
                args.insert("next_link".to_string(), json!(nl));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            if response_format != "concise" {
                args.insert("response_format".to_string(), json!(response_format));
            }
            ("list_messages", args)
        }
        MicrosoftGraphTools::ListEvents {
            days_ahead,
            limit,
            next_link,
            response_format,
        } => {
            let mut args = Map::new();
            if days_ahead != 7 {
                args.insert("days_ahead".to_string(), json!(days_ahead));
            }
            if limit != 25 {
                args.insert("limit".to_string(), json!(limit));
            }
            if let Some(nl) = next_link {
                args.insert("next_link".to_string(), json!(nl));
            }
            let response_format = if response_format == "full" {
                "detailed".to_string()
            } else {
                response_format
            };
            if response_format != "concise" {
                args.insert("response_format".to_string(), json!(response_format));
            }
            ("list_events", args)
        }
        MicrosoftGraphTools::GetMessage { message_id } => {
            let mut args = Map::new();
            args.insert("message_id".to_string(), json!(message_id));
            ("get_message", args)
        }
        MicrosoftGraphTools::SendMail { to, subject, body } => {
            let mut args = Map::new();
            // Parse comma-separated emails into array
            let to_array: Vec<String> = to.split(',').map(|s| s.trim().to_string()).collect();
            args.insert("to".to_string(), json!(to_array));
            args.insert("subject".to_string(), json!(subject));
            args.insert("body_text".to_string(), json!(body));
            ("send_mail", args)
        }
        MicrosoftGraphTools::CreateDraft { to, subject, body } => {
            let mut args = Map::new();
            // Parse comma-separated emails into array
            let to_array: Vec<String> = to.split(',').map(|s| s.trim().to_string()).collect();
            args.insert("to".to_string(), json!(to_array));
            args.insert("subject".to_string(), json!(subject));
            args.insert("body_text".to_string(), json!(body));
            ("create_draft", args)
        }
        MicrosoftGraphTools::UploadAttachment {
            message_id,
            filename,
            mime_type,
            data_base64,
        } => {
            let mut args = Map::new();
            args.insert("message_id".to_string(), json!(message_id));
            args.insert("filename".to_string(), json!(filename));
            args.insert("mime_type".to_string(), json!(mime_type));
            args.insert("data_base64".to_string(), json!(data_base64));
            ("upload_attachment_large", args)
        }
        MicrosoftGraphTools::SendDraft { message_id } => {
            let mut args = Map::new();
            args.insert("message_id".to_string(), json!(message_id));
            ("send_draft", args)
        }
        MicrosoftGraphTools::UploadAttachmentFromPath {
            message_id,
            file_path,
            filename,
            mime_type,
        } => {
            let mut args = Map::new();
            args.insert("message_id".to_string(), json!(message_id));
            args.insert("file_path".to_string(), json!(file_path));
            if let Some(f) = filename {
                args.insert("filename".to_string(), json!(f));
            }
            if let Some(m) = mime_type {
                args.insert("mime_type".to_string(), json!(m));
            }
            ("upload_attachment_large_from_path", args)
        }
        MicrosoftGraphTools::AuthStart {
            tenant_id,
            client_id,
            scopes,
        } => {
            let mut args = Map::new();
            if let Some(t) = tenant_id {
                args.insert("tenant_id".to_string(), json!(t));
            }
            if let Some(c) = client_id {
                args.insert("client_id".to_string(), json!(c));
            }
            if let Some(s) = scopes {
                args.insert("scopes".to_string(), json!(s));
            }
            ("auth_start", args)
        }
        MicrosoftGraphTools::AuthPoll {
            tenant_id,
            client_id,
            device_code,
        } => {
            let mut args = Map::new();
            if let Some(t) = tenant_id {
                args.insert("tenant_id".to_string(), json!(t));
            }
            args.insert("client_id".to_string(), json!(client_id));
            args.insert("device_code".to_string(), json!(device_code));
            ("auth_poll", args)
        }
    };

    call_tool(cli, "microsoft-graph", tool_name, args).await
}

/// Handle IMAP commands
pub async fn handle_imap(cli: &Cli, tool: ImapTools) -> Result<()> {
    let (tool_name, args) = match tool {
        ImapTools::ListMailboxes {
            reference,
            pattern,
            include_subscribed,
        } => {
            let mut args = Map::new();
            if let Some(r) = reference {
                args.insert("reference".to_string(), json!(r));
            }
            if pattern != "*" {
                args.insert("pattern".to_string(), json!(pattern));
            }
            if include_subscribed {
                args.insert("include_subscribed".to_string(), json!(include_subscribed));
            }
            ("list_mailboxes", args)
        }
        ImapTools::FetchMessages {
            mailbox,
            limit,
            offset,
            before_uid,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert("limit".to_string(), json!(limit));
            if let Some(o) = offset {
                args.insert("offset".to_string(), json!(o));
            }
            if let Some(b) = before_uid {
                args.insert("before_uid".to_string(), json!(b));
            }
            ("fetch_messages", args)
        }
        ImapTools::GetMessage {
            mailbox,
            uid,
            include_headers,
            include_html,
            include_raw,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert("uid".to_string(), json!(uid));
            if include_headers {
                args.insert("include_headers".to_string(), json!(include_headers));
            }
            if include_html {
                args.insert("include_html".to_string(), json!(include_html));
            }
            if include_raw {
                args.insert("include_raw".to_string(), json!(include_raw));
            }
            ("get_message", args)
        }
        ImapTools::Search {
            mailbox,
            query,
            limit,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        ImapTools::MoveMessages {
            mailbox,
            destination_mailbox,
            uids,
            apply,
            allow_expunge_all,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert(
                "destination_mailbox".to_string(),
                json!(destination_mailbox),
            );
            args.insert("uids".to_string(), json!(uids));
            // Default to dry-run for safety; require explicit --apply to mutate mailboxes.
            args.insert("dry_run".to_string(), json!(!apply));
            if allow_expunge_all {
                args.insert("allow_expunge_all".to_string(), json!(allow_expunge_all));
            }
            ("move_messages", args)
        }
        ImapTools::DeleteMessages {
            mailbox,
            uids,
            expunge,
            apply,
            allow_expunge_all,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert("uids".to_string(), json!(uids));
            if expunge {
                args.insert("expunge".to_string(), json!(expunge));
            }
            // Default to dry-run for safety; require explicit --apply to mutate mailboxes.
            args.insert("dry_run".to_string(), json!(!apply));
            if allow_expunge_all {
                args.insert("allow_expunge_all".to_string(), json!(allow_expunge_all));
            }
            ("delete_messages", args)
        }
        ImapTools::AddFlags {
            mailbox,
            uids,
            flags,
            apply,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert("uids".to_string(), json!(uids));
            args.insert("flags".to_string(), json!(flags));
            args.insert("dry_run".to_string(), json!(!apply));
            ("add_flags", args)
        }
        ImapTools::RemoveFlags {
            mailbox,
            uids,
            flags,
            apply,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert("uids".to_string(), json!(uids));
            args.insert("flags".to_string(), json!(flags));
            args.insert("dry_run".to_string(), json!(!apply));
            ("remove_flags", args)
        }
        ImapTools::MarkSeen {
            mailbox,
            uids,
            apply,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert("uids".to_string(), json!(uids));
            args.insert("flags".to_string(), json!(vec!["\\Seen"]));
            args.insert("dry_run".to_string(), json!(!apply));
            ("add_flags", args)
        }
        ImapTools::MarkUnseen {
            mailbox,
            uids,
            apply,
        } => {
            let mut args = Map::new();
            if let Some(m) = mailbox {
                args.insert("mailbox".to_string(), json!(m));
            }
            args.insert("uids".to_string(), json!(uids));
            args.insert("flags".to_string(), json!(vec!["\\Seen"]));
            args.insert("dry_run".to_string(), json!(!apply));
            ("remove_flags", args)
        }
    };

    call_tool(cli, "imap", tool_name, args).await
}

/// Handle SMTP commands
pub async fn handle_smtp(cli: &Cli, tool: SmtpTools) -> Result<()> {
    let (tool_name, args) = match tool {
        SmtpTools::SendMail {
            to,
            subject,
            body,
            html_body,
            from,
            reply_to,
            cc,
            bcc,
            dry_run,
        } => {
            let mut args = Map::new();
            args.insert("to".to_string(), json!(to));
            args.insert("subject".to_string(), json!(subject));
            args.insert("body".to_string(), json!(body));
            if let Some(html) = html_body {
                args.insert("html_body".to_string(), json!(html));
            }
            if let Some(from) = from {
                args.insert("from".to_string(), json!(from));
            }
            if let Some(reply_to) = reply_to {
                args.insert("reply_to".to_string(), json!(reply_to));
            }
            if let Some(cc) = cc {
                args.insert("cc".to_string(), json!(cc));
            }
            if let Some(bcc) = bcc {
                args.insert("bcc".to_string(), json!(bcc));
            }
            if dry_run {
                args.insert("dry_run".to_string(), json!(true));
            }
            ("send_mail", args)
        }
        SmtpTools::TestConnection => ("test_connection", Map::new()),
    };

    call_tool(cli, "smtp", tool_name, args).await
}

/// Handle localfs commands
pub async fn handle_localfs(cli: &Cli, tool: LocalfsTools) -> Result<()> {
    let (tool_name, args) = match tool {
        LocalfsTools::ListFiles {
            path,
            recursive,
            extensions,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("path".to_string(), json!(path));
            args.insert("recursive".to_string(), json!(recursive));
            if let Some(ext) = extensions {
                args.insert("extensions".to_string(), json!(ext));
            }
            args.insert("limit".to_string(), json!(limit));
            ("list_files", args)
        }
        LocalfsTools::FileInfo { path } => {
            let mut args = Map::new();
            args.insert("path".to_string(), json!(path));
            ("get_file_info", args)
        }
        LocalfsTools::Structure { path } => {
            let mut args = Map::new();
            args.insert("path".to_string(), json!(path));
            ("get_structure", args)
        }
        LocalfsTools::ExtractText {
            path,
            format,
            max_chars,
        } => {
            let mut args = Map::new();
            args.insert("path".to_string(), json!(path));
            args.insert("format".to_string(), json!(format));
            if let Some(m) = max_chars {
                args.insert("max_chars".to_string(), json!(m));
            }
            ("extract_text", args)
        }
        LocalfsTools::Section {
            path,
            section,
            max_chars,
        } => {
            let mut args = Map::new();
            args.insert("path".to_string(), json!(path));
            args.insert("section".to_string(), json!(section));
            if let Some(m) = max_chars {
                args.insert("max_chars".to_string(), json!(m));
            }
            ("get_section", args)
        }
        LocalfsTools::Search {
            path,
            query,
            context,
        } => {
            let mut args = Map::new();
            args.insert("path".to_string(), json!(path));
            args.insert("query".to_string(), json!(query));
            args.insert("context_lines".to_string(), json!(context));
            ("search_content", args)
        }
    };

    call_tool(cli, "localfs", tool_name, args).await
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
        YoutubeTools::Transcript { id_or_url, id } => {
            let id = id_or_url.or(id).ok_or_else(|| {
                crate::commands::CommandError::InvalidInput(
                    "Missing video ID/URL. Use `rzn-tools youtube get`.".to_string(),
                )
            })?;

            let mut tool_args = Map::new();
            tool_args.insert("video_id".to_string(), json!(id));
            tool_args.insert("response_format".to_string(), json!("concise"));

            let (payload, meta_value) = call_tool_raw(cli, "youtube", "get", tool_args).await?;
            let transcript_only = payload.get("transcript").cloned().unwrap_or(Value::Null);
            output_tool_result(
                cli,
                "youtube",
                "transcript",
                &transcript_only,
                meta_value.as_ref(),
            )
        }
        YoutubeTools::Chapters { id_or_url, id } => {
            let id = id_or_url.or(id).ok_or_else(|| {
                crate::commands::CommandError::InvalidInput(
                    "Missing video ID/URL. Use `rzn-tools youtube get`.".to_string(),
                )
            })?;

            let mut tool_args = Map::new();
            tool_args.insert("video_id".to_string(), json!(id));
            tool_args.insert("response_format".to_string(), json!("concise"));

            let (payload, meta_value) = call_tool_raw(cli, "youtube", "get", tool_args).await?;
            let chapters_only = payload
                .get("chapters")
                .cloned()
                .unwrap_or(Value::Array(Vec::new()));
            output_tool_result(
                cli,
                "youtube",
                "chapters",
                &chapters_only,
                meta_value.as_ref(),
            )
        }
    }
}

/// Handle hackernews commands
pub async fn handle_hackernews(cli: &Cli, tool: HackernewsTools) -> Result<()> {
    let (tool_name, args) = match tool {
        HackernewsTools::Search { query, limit } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        HackernewsTools::Story {
            id,
            max_comments,
            response_format,
        } => {
            let mut args = Map::new();
            args.insert("id".to_string(), json!(id));
            args.insert("max_comments".to_string(), json!(max_comments));
            args.insert("response_format".to_string(), json!(response_format));
            ("get_thread", args)
        }
        HackernewsTools::Top { limit } => {
            let mut args = Map::new();
            args.insert("feed".to_string(), json!("top"));
            args.insert("limit".to_string(), json!(limit));
            ("list_threads", args)
        }
        HackernewsTools::New { limit } => {
            let mut args = Map::new();
            args.insert("feed".to_string(), json!("new"));
            args.insert("limit".to_string(), json!(limit));
            ("list_threads", args)
        }
        HackernewsTools::Best { limit } => {
            let mut args = Map::new();
            args.insert("feed".to_string(), json!("best"));
            args.insert("limit".to_string(), json!(limit));
            ("list_threads", args)
        }
        HackernewsTools::Comments { id, limit } => {
            let mut args = Map::new();
            args.insert("id".to_string(), json!(id));
            args.insert("max_comments".to_string(), json!(limit));
            args.insert("response_format".to_string(), json!("compact"));
            ("get_thread", args)
        }
    };

    call_tool(cli, "hackernews", tool_name, args).await
}

/// Handle arxiv commands
pub async fn handle_arxiv(cli: &Cli, tool: ArxivTools) -> Result<()> {
    let (tool_name, args) = match tool {
        ArxivTools::Search { query, limit, sort } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            args.insert("sort_by".to_string(), json!(sort));
            ("search", args)
        }
        ArxivTools::Paper { id } => {
            let mut args = Map::new();
            args.insert("paper_id".to_string(), json!(id));
            args.insert("response_format".to_string(), json!("detailed"));
            ("get", args)
        }
        ArxivTools::Pdf { id } => {
            let mut tool_args = Map::new();
            tool_args.insert("paper_id".to_string(), json!(id));
            tool_args.insert("response_format".to_string(), json!("concise"));

            let (payload, meta_value) = call_tool_raw(cli, "arxiv", "get", tool_args).await?;
            let pdf_url_only = payload.get("pdf_url").cloned().unwrap_or(Value::Null);
            return output_tool_result(cli, "arxiv", "pdf", &pdf_url_only, meta_value.as_ref());
        }
    };

    call_tool(cli, "arxiv", tool_name, args).await
}

/// Handle github commands
pub async fn handle_github(cli: &Cli, tool: GithubTools) -> Result<()> {
    fn split_owner_repo(repo: &str) -> Result<(String, String)> {
        let (owner, name) = repo.split_once('/').ok_or_else(|| {
            crate::commands::CommandError::InvalidInput(
                "Invalid repo. Expected 'owner/repo' (e.g., rust-lang/rust).".to_string(),
            )
        })?;
        Ok((owner.to_string(), name.to_string()))
    }

    let (tool_name, args) = match tool {
        GithubTools::SearchRepos { query, limit } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("per_page".to_string(), json!(limit));
            args.insert("page".to_string(), json!(1));
            ("search_repositories", args)
        }
        GithubTools::SearchCode { query, repo, limit } => {
            let mut args = Map::new();
            let query = if let Some(r) = repo {
                format!("{} repo:{}", query, r)
            } else {
                query
            };
            args.insert("query".to_string(), json!(query));
            args.insert("per_page".to_string(), json!(limit));
            args.insert("page".to_string(), json!(1));
            ("code_search", args)
        }
        GithubTools::Issues { repo, state, limit } => {
            let (owner, name) = split_owner_repo(&repo)?;
            let mut args = Map::new();
            args.insert("owner".to_string(), json!(owner));
            args.insert("repo".to_string(), json!(name));
            args.insert("state".to_string(), json!(state));
            args.insert("per_page".to_string(), json!(limit));
            args.insert("page".to_string(), json!(1));
            ("list_issues", args)
        }
        GithubTools::Pulls { repo, state, limit } => {
            let (owner, name) = split_owner_repo(&repo)?;
            let mut args = Map::new();
            args.insert("owner".to_string(), json!(owner));
            args.insert("repo".to_string(), json!(name));
            args.insert("state".to_string(), json!(state));
            args.insert("per_page".to_string(), json!(limit));
            args.insert("page".to_string(), json!(1));
            ("list_pull_requests", args)
        }
        GithubTools::Repo { repo } => {
            let (owner, name) = split_owner_repo(&repo)?;
            let mut args = Map::new();
            args.insert("owner".to_string(), json!(owner));
            args.insert("repo".to_string(), json!(name));
            ("get_repository", args)
        }
    };

    call_tool(cli, "github", tool_name, args).await
}

/// Handle reddit commands
pub async fn handle_reddit(cli: &Cli, tool: RedditTools) -> Result<()> {
    let (tool_name, args) = match tool {
        RedditTools::Search {
            query,
            subreddit,
            sort,
            time,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            if let Some(sub) = subreddit {
                args.insert("subreddit".to_string(), json!(sub));
            }
            if sort != "relevance" {
                args.insert("sort".to_string(), json!(sort));
            }
            if time != "all" {
                args.insert("time".to_string(), json!(time));
            }
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        RedditTools::Hot {
            subreddit,
            limit,
            cursor,
            output_format,
            include_nsfw,
        } => {
            let mut args = Map::new();
            args.insert("subreddit".to_string(), json!(subreddit));
            args.insert("limit".to_string(), json!(limit));
            args.insert("sort".to_string(), json!("hot"));
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            if include_nsfw {
                args.insert("include_nsfw".to_string(), json!(true));
            }
            ("list", args)
        }
        RedditTools::New {
            subreddit,
            limit,
            cursor,
            output_format,
            include_nsfw,
        } => {
            let mut args = Map::new();
            args.insert("subreddit".to_string(), json!(subreddit));
            args.insert("limit".to_string(), json!(limit));
            args.insert("sort".to_string(), json!("new"));
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            if include_nsfw {
                args.insert("include_nsfw".to_string(), json!(true));
            }
            ("list", args)
        }
        RedditTools::Top {
            subreddit,
            time,
            limit,
            cursor,
            output_format,
            include_nsfw,
        } => {
            let mut args = Map::new();
            args.insert("subreddit".to_string(), json!(subreddit));
            args.insert("limit".to_string(), json!(limit));
            args.insert("sort".to_string(), json!("top"));
            args.insert("time".to_string(), json!(time));
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            if include_nsfw {
                args.insert("include_nsfw".to_string(), json!(true));
            }
            ("list", args)
        }
        RedditTools::Media { id, include_nsfw } => {
            let mut args = Map::new();
            args.insert("id".to_string(), json!(id));
            if include_nsfw {
                args.insert("include_nsfw".to_string(), json!(true));
            }
            ("media", args)
        }
        RedditTools::Post {
            id,
            comment_limit,
            comment_sort,
        } => {
            let mut args = Map::new();
            let post_url = if id.starts_with("http://") || id.starts_with("https://") {
                id
            } else {
                format!("https://www.reddit.com/comments/{}", id)
            };
            args.insert("post_url".to_string(), json!(post_url));
            args.insert("comment_limit".to_string(), json!(comment_limit));
            if comment_sort != "best" {
                args.insert("comment_sort".to_string(), json!(comment_sort));
            }
            ("get", args)
        }
        RedditTools::User {
            username,
            output_format,
        } => {
            let mut args = Map::new();
            args.insert("username".to_string(), json!(username));
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            ("user", args)
        }
    };

    call_tool(cli, "reddit", tool_name, args).await
}

/// Handle Polymarket commands
pub async fn handle_polymarket(cli: &Cli, tool: PolymarketTools) -> Result<()> {
    let (tool_name, args) = match tool {
        PolymarketTools::ListTags {
            limit,
            offset,
            cursor,
        } => {
            let mut args = Map::new();
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if offset != 0 {
                args.insert("offset".to_string(), json!(offset));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            ("list_tags", args)
        }
        PolymarketTools::ListEvents {
            limit,
            offset,
            cursor,
            series_id,
            series_slug,
            tag_slug,
            active,
            closed,
            archived,
            featured,
        } => {
            let mut args = Map::new();
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if offset != 0 {
                args.insert("offset".to_string(), json!(offset));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if let Some(series_id) = series_id {
                args.insert("series_id".to_string(), json!(series_id));
            }
            if let Some(series_slug) = series_slug {
                args.insert("series_slug".to_string(), json!(series_slug));
            }
            if let Some(tag_slug) = tag_slug {
                args.insert("tag_slug".to_string(), json!(tag_slug));
            }
            if active {
                args.insert("active".to_string(), json!(true));
            }
            if closed {
                args.insert("closed".to_string(), json!(true));
            }
            if archived {
                args.insert("archived".to_string(), json!(true));
            }
            if featured {
                args.insert("featured".to_string(), json!(true));
            }
            ("list_events", args)
        }
        PolymarketTools::ListMarkets {
            limit,
            offset,
            cursor,
            slug,
            event_item_ref,
            event_id,
            event_slug,
            series_id,
            series_slug,
            tag_slug,
            active,
            closed,
        } => {
            let mut args = Map::new();
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if offset != 0 {
                args.insert("offset".to_string(), json!(offset));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if let Some(slug) = slug {
                args.insert("slug".to_string(), json!(slug));
            }
            if let Some(event_item_ref) = event_item_ref {
                args.insert("event_item_ref".to_string(), json!(event_item_ref));
            }
            if let Some(event_id) = event_id {
                args.insert("event_id".to_string(), json!(event_id));
            }
            if let Some(event_slug) = event_slug {
                args.insert("event_slug".to_string(), json!(event_slug));
            }
            if let Some(series_id) = series_id {
                args.insert("series_id".to_string(), json!(series_id));
            }
            if let Some(series_slug) = series_slug {
                args.insert("series_slug".to_string(), json!(series_slug));
            }
            if let Some(tag_slug) = tag_slug {
                args.insert("tag_slug".to_string(), json!(tag_slug));
            }
            if active {
                args.insert("active".to_string(), json!(true));
            }
            if closed {
                args.insert("closed".to_string(), json!(true));
            }
            ("list_markets", args)
        }
        PolymarketTools::ListSeries {
            limit,
            offset,
            cursor,
            slug,
            active,
            closed,
            featured,
        } => {
            let mut args = Map::new();
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if offset != 0 {
                args.insert("offset".to_string(), json!(offset));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if let Some(slug) = slug {
                args.insert("slug".to_string(), json!(slug));
            }
            if active {
                args.insert("active".to_string(), json!(true));
            }
            if closed {
                args.insert("closed".to_string(), json!(true));
            }
            if featured {
                args.insert("featured".to_string(), json!(true));
            }
            ("list_series", args)
        }
        PolymarketTools::GetSeries { id, slug } => {
            let mut args = Map::new();
            if let Some(id) = id {
                args.insert("id".to_string(), json!(id));
            }
            if let Some(slug) = slug {
                args.insert("slug".to_string(), json!(slug));
            }
            ("get_series", args)
        }
        PolymarketTools::ListComments {
            limit,
            offset,
            cursor,
            item_ref,
            event_url,
            event_id,
            event_slug,
            market_id,
            market_slug,
            series_id,
            series_slug,
        } => {
            let mut args = Map::new();
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if offset != 0 {
                args.insert("offset".to_string(), json!(offset));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(event_url) = event_url {
                args.insert("event_url".to_string(), json!(event_url));
            }
            if let Some(event_id) = event_id {
                args.insert("event_id".to_string(), json!(event_id));
            }
            if let Some(event_slug) = event_slug {
                args.insert("event_slug".to_string(), json!(event_slug));
            }
            if let Some(market_id) = market_id {
                args.insert("market_id".to_string(), json!(market_id));
            }
            if let Some(market_slug) = market_slug {
                args.insert("market_slug".to_string(), json!(market_slug));
            }
            if let Some(series_id) = series_id {
                args.insert("series_id".to_string(), json!(series_id));
            }
            if let Some(series_slug) = series_slug {
                args.insert("series_slug".to_string(), json!(series_slug));
            }
            ("list_comments", args)
        }
        PolymarketTools::OrderBook {
            item_ref,
            id,
            slug,
            outcome,
            token_id,
            depth,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(id) = id {
                args.insert("id".to_string(), json!(id));
            }
            if let Some(slug) = slug {
                args.insert("slug".to_string(), json!(slug));
            }
            if let Some(outcome) = outcome {
                args.insert("outcome".to_string(), json!(outcome));
            }
            if let Some(token_id) = token_id {
                args.insert("token_id".to_string(), json!(token_id));
            }
            if depth != 5 {
                args.insert("depth".to_string(), json!(depth));
            }
            ("order_book", args)
        }
        PolymarketTools::PriceHistory {
            item_ref,
            id,
            slug,
            outcome,
            token_id,
            interval,
            fidelity,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(id) = id {
                args.insert("id".to_string(), json!(id));
            }
            if let Some(slug) = slug {
                args.insert("slug".to_string(), json!(slug));
            }
            if let Some(outcome) = outcome {
                args.insert("outcome".to_string(), json!(outcome));
            }
            if let Some(token_id) = token_id {
                args.insert("token_id".to_string(), json!(token_id));
            }
            if interval != "1d" {
                args.insert("interval".to_string(), json!(interval));
            }
            if fidelity != 60 {
                args.insert("fidelity".to_string(), json!(fidelity));
            }
            ("price_history", args)
        }
        PolymarketTools::MarketPositions {
            item_ref,
            id,
            slug,
            limit,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(id) = id {
                args.insert("id".to_string(), json!(id));
            }
            if let Some(slug) = slug {
                args.insert("slug".to_string(), json!(slug));
            }
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            ("market_positions", args)
        }
        PolymarketTools::MarketContext {
            item_ref,
            id,
            slug,
            depth,
            interval,
            fidelity,
            include_positions,
            positions_limit,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(id) = id {
                args.insert("id".to_string(), json!(id));
            }
            if let Some(slug) = slug {
                args.insert("slug".to_string(), json!(slug));
            }
            if depth != 5 {
                args.insert("depth".to_string(), json!(depth));
            }
            if interval != "1d" {
                args.insert("interval".to_string(), json!(interval));
            }
            if fidelity != 60 {
                args.insert("fidelity".to_string(), json!(fidelity));
            }
            if include_positions {
                args.insert("include_positions".to_string(), json!(true));
            }
            if positions_limit != 20 {
                args.insert("positions_limit".to_string(), json!(positions_limit));
            }
            ("get_market_context", args)
        }
    };

    call_tool(cli, "polymarket", tool_name, args).await
}

/// Handle Kalshi commands
pub async fn handle_kalshi(cli: &Cli, tool: KalshiTools) -> Result<()> {
    let (tool_name, args) = match tool {
        KalshiTools::Search {
            query,
            limit,
            output_format,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            if limit != 10 {
                args.insert("limit".to_string(), json!(limit));
            }
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            ("search", args)
        }
        KalshiTools::ListSeries {
            limit,
            cursor,
            status,
        } => {
            let mut args = Map::new();
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if let Some(status) = status {
                args.insert("status".to_string(), json!(status));
            }
            ("list_series", args)
        }
        KalshiTools::GetSeries {
            item_ref,
            ticker,
            events_limit,
            output_format,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if events_limit != 10 {
                args.insert("events_limit".to_string(), json!(events_limit));
            }
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            ("get_series", args)
        }
        KalshiTools::ListEvents {
            limit,
            cursor,
            series_ticker,
            status,
            multivariate,
            collection_ticker,
        } => {
            let mut args = Map::new();
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if let Some(series_ticker) = series_ticker {
                args.insert("series_ticker".to_string(), json!(series_ticker));
            }
            if let Some(status) = status {
                args.insert("status".to_string(), json!(status));
            }
            if multivariate {
                args.insert("multivariate".to_string(), json!(true));
            }
            if let Some(collection_ticker) = collection_ticker {
                args.insert("collection_ticker".to_string(), json!(collection_ticker));
            }
            ("list_events", args)
        }
        KalshiTools::GetEvent {
            item_ref,
            ticker,
            url,
            output_format,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if let Some(url) = url {
                args.insert("url".to_string(), json!(url));
            }
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            ("get", args)
        }
        KalshiTools::EventMetadata {
            item_ref,
            ticker,
            url,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if let Some(url) = url {
                args.insert("url".to_string(), json!(url));
            }
            ("get_event_metadata", args)
        }
        KalshiTools::EventCandles {
            item_ref,
            ticker,
            url,
            series_ticker,
            start_ts,
            end_ts,
            period_interval,
        } => {
            let mut args = Map::new();
            args.insert("start_ts".to_string(), json!(start_ts));
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if let Some(url) = url {
                args.insert("url".to_string(), json!(url));
            }
            if let Some(series_ticker) = series_ticker {
                args.insert("series_ticker".to_string(), json!(series_ticker));
            }
            if let Some(end_ts) = end_ts {
                args.insert("end_ts".to_string(), json!(end_ts));
            }
            if period_interval != 60 {
                args.insert("period_interval".to_string(), json!(period_interval));
            }
            ("event_candlesticks", args)
        }
        KalshiTools::ListMarkets {
            limit,
            cursor,
            series_ticker,
            event_ticker,
            status,
            historical,
        } => {
            let mut args = Map::new();
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if let Some(series_ticker) = series_ticker {
                args.insert("series_ticker".to_string(), json!(series_ticker));
            }
            if let Some(event_ticker) = event_ticker {
                args.insert("event_ticker".to_string(), json!(event_ticker));
            }
            if let Some(status) = status {
                args.insert("status".to_string(), json!(status));
            }
            if historical {
                args.insert("historical".to_string(), json!(true));
            }
            ("list_markets", args)
        }
        KalshiTools::GetMarket {
            item_ref,
            ticker,
            output_format,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            ("get_market", args)
        }
        KalshiTools::OrderBook {
            item_ref,
            ticker,
            depth,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if depth != 10 {
                args.insert("depth".to_string(), json!(depth));
            }
            ("order_book", args)
        }
        KalshiTools::MarketCandles {
            item_ref,
            ticker,
            start_ts,
            end_ts,
            period_interval,
        } => {
            let mut args = Map::new();
            args.insert("start_ts".to_string(), json!(start_ts));
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if let Some(end_ts) = end_ts {
                args.insert("end_ts".to_string(), json!(end_ts));
            }
            if period_interval != 60 {
                args.insert("period_interval".to_string(), json!(period_interval));
            }
            ("market_candlesticks", args)
        }
        KalshiTools::ListTrades {
            item_ref,
            ticker,
            limit,
            cursor,
            min_ts,
            max_ts,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if limit != 20 {
                args.insert("limit".to_string(), json!(limit));
            }
            if let Some(cursor) = cursor {
                args.insert("cursor".to_string(), json!(cursor));
            }
            if let Some(min_ts) = min_ts {
                args.insert("min_ts".to_string(), json!(min_ts));
            }
            if let Some(max_ts) = max_ts {
                args.insert("max_ts".to_string(), json!(max_ts));
            }
            ("list_trades", args)
        }
        KalshiTools::MarketContext {
            item_ref,
            ticker,
            start_ts,
            end_ts,
            period_interval,
            orderbook_depth,
            trades_limit,
            skip_event_metadata,
        } => {
            let mut args = Map::new();
            if let Some(item_ref) = item_ref {
                args.insert("item_ref".to_string(), json!(item_ref));
            }
            if let Some(ticker) = ticker {
                args.insert("ticker".to_string(), json!(ticker));
            }
            if let Some(start_ts) = start_ts {
                args.insert("start_ts".to_string(), json!(start_ts));
            }
            if let Some(end_ts) = end_ts {
                args.insert("end_ts".to_string(), json!(end_ts));
            }
            if period_interval != 60 {
                args.insert("period_interval".to_string(), json!(period_interval));
            }
            if orderbook_depth != 10 {
                args.insert("orderbook_depth".to_string(), json!(orderbook_depth));
            }
            if trades_limit != 20 {
                args.insert("trades_limit".to_string(), json!(trades_limit));
            }
            if skip_event_metadata {
                args.insert("include_event_metadata".to_string(), json!(false));
            }
            ("get_market_context", args)
        }
    };

    call_tool(cli, "kalshi", tool_name, args).await
}

/// Handle Play Store commands
pub async fn handle_play_store(cli: &Cli, tool: PlayStoreTools) -> Result<()> {
    let (tool_name, args) = match tool {
        PlayStoreTools::App {
            id,
            hl,
            gl,
            output_format,
        } => {
            let mut args = Map::new();
            args.insert("id".to_string(), json!(id));
            if hl != "en" {
                args.insert("hl".to_string(), json!(hl));
            }
            if gl != "US" {
                args.insert("gl".to_string(), json!(gl));
            }
            if output_format != "raw" {
                args.insert("output_format".to_string(), json!(output_format));
            }
            ("app", args)
        }
    };

    call_tool(cli, "play-store", tool_name, args).await
}

/// Handle App Store commands
pub async fn handle_app_store(cli: &Cli, tool: AppStoreTools) -> Result<()> {
    let (tool_name, args) = match tool {
        AppStoreTools::Search {
            query,
            country,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            if country != "US" {
                args.insert("country".to_string(), json!(country));
            }
            if limit != 25 {
                args.insert("limit".to_string(), json!(limit));
            }
            ("search", args)
        }
        AppStoreTools::Lookup { track_id, country } => {
            let mut args = Map::new();
            args.insert("track_id".to_string(), json!(track_id));
            if country != "US" {
                args.insert("country".to_string(), json!(country));
            }
            ("lookup", args)
        }
        AppStoreTools::Reviews { track_id } => {
            let mut args = Map::new();
            args.insert("track_id".to_string(), json!(track_id));
            ("reviews", args)
        }
        AppStoreTools::TestAuth => ("test_auth", Map::new()),
    };

    call_tool(cli, "app-store", tool_name, args).await
}

/// Handle App Store Connect commands
pub async fn handle_app_store_connect(cli: &Cli, tool: AppStoreConnectTools) -> Result<()> {
    let (tool_name, args) = match tool {
        AppStoreConnectTools::ListApps {
            limit,
            filter_name,
            filter_bundle_id,
            filter_sku,
        } => {
            let mut args = Map::new();
            if limit != 100 {
                args.insert("limit".to_string(), json!(limit));
            }
            if let Some(v) = filter_name {
                args.insert("filter_name".to_string(), json!(v));
            }
            if let Some(v) = filter_bundle_id {
                args.insert("filter_bundle_id".to_string(), json!(v));
            }
            if let Some(v) = filter_sku {
                args.insert("filter_sku".to_string(), json!(v));
            }
            ("list_apps", args)
        }
        AppStoreConnectTools::GetApp { app_id } => {
            let mut args = Map::new();
            args.insert("app_id".to_string(), json!(app_id));
            ("get_app", args)
        }
        AppStoreConnectTools::CreateAnalyticsReportRequest {
            app_id,
            access_type,
        } => {
            let mut args = Map::new();
            args.insert("app_id".to_string(), json!(app_id));
            if access_type != "ONE_TIME_SNAPSHOT" {
                args.insert("access_type".to_string(), json!(access_type));
            }
            ("create_analytics_report_request", args)
        }
        AppStoreConnectTools::ListAnalyticsReports {
            report_request_id,
            limit,
            filter_category,
            filter_name,
        } => {
            let mut args = Map::new();
            args.insert("report_request_id".to_string(), json!(report_request_id));
            if limit != 100 {
                args.insert("limit".to_string(), json!(limit));
            }
            if let Some(v) = filter_category {
                args.insert("filter_category".to_string(), json!(v));
            }
            if let Some(v) = filter_name {
                args.insert("filter_name".to_string(), json!(v));
            }
            ("list_analytics_reports", args)
        }
        AppStoreConnectTools::ListAnalyticsReportInstances {
            report_id,
            limit,
            filter_processing_date,
            filter_granularity,
        } => {
            let mut args = Map::new();
            args.insert("report_id".to_string(), json!(report_id));
            if limit != 100 {
                args.insert("limit".to_string(), json!(limit));
            }
            if let Some(v) = filter_processing_date {
                args.insert("filter_processing_date".to_string(), json!(v));
            }
            if let Some(v) = filter_granularity {
                args.insert("filter_granularity".to_string(), json!(v));
            }
            ("list_analytics_report_instances", args)
        }
        AppStoreConnectTools::ListAnalyticsReportSegments { instance_id, limit } => {
            let mut args = Map::new();
            args.insert("instance_id".to_string(), json!(instance_id));
            if limit != 100 {
                args.insert("limit".to_string(), json!(limit));
            }
            ("list_analytics_report_segments", args)
        }
        AppStoreConnectTools::DownloadAnalyticsReportSegment {
            segment_url,
            segment_id,
            max_kb,
            max_uncompressed_kb,
            max_rows,
            max_preview_chars,
        } => {
            let mut args = Map::new();
            if let Some(v) = segment_url {
                args.insert("segment_url".to_string(), json!(v));
            }
            if let Some(v) = segment_id {
                args.insert("segment_id".to_string(), json!(v));
            }
            if let Some(v) = max_kb {
                args.insert("max_kb".to_string(), json!(v));
            }
            if let Some(v) = max_uncompressed_kb {
                args.insert("max_uncompressed_kb".to_string(), json!(v));
            }
            if let Some(v) = max_rows {
                args.insert("max_rows".to_string(), json!(v));
            }
            if let Some(v) = max_preview_chars {
                args.insert("max_preview_chars".to_string(), json!(v));
            }
            ("download_analytics_report_segment", args)
        }
        AppStoreConnectTools::DownloadSalesReport {
            vendor_number,
            report_type,
            report_sub_type,
            frequency,
            report_date,
            version,
            max_kb,
            max_uncompressed_kb,
            max_rows,
            max_preview_chars,
        } => {
            let mut args = Map::new();
            if let Some(v) = vendor_number {
                args.insert("vendor_number".to_string(), json!(v));
            }
            if report_type != "SALES" {
                args.insert("report_type".to_string(), json!(report_type));
            }
            if report_sub_type != "SUMMARY" {
                args.insert("report_sub_type".to_string(), json!(report_sub_type));
            }
            if frequency != "MONTHLY" {
                args.insert("frequency".to_string(), json!(frequency));
            }
            if let Some(v) = report_date {
                args.insert("report_date".to_string(), json!(v));
            }
            if let Some(v) = version {
                args.insert("version".to_string(), json!(v));
            }
            if let Some(v) = max_kb {
                args.insert("max_kb".to_string(), json!(v));
            }
            if let Some(v) = max_uncompressed_kb {
                args.insert("max_uncompressed_kb".to_string(), json!(v));
            }
            if let Some(v) = max_rows {
                args.insert("max_rows".to_string(), json!(v));
            }
            if let Some(v) = max_preview_chars {
                args.insert("max_preview_chars".to_string(), json!(v));
            }
            ("download_sales_report", args)
        }
        AppStoreConnectTools::DownloadFinanceReport {
            vendor_number,
            report_type,
            report_date,
            region_code,
            max_kb,
            max_uncompressed_kb,
            max_rows,
            max_preview_chars,
        } => {
            let mut args = Map::new();
            if let Some(v) = vendor_number {
                args.insert("vendor_number".to_string(), json!(v));
            }
            if report_type != "FINANCIAL" {
                args.insert("report_type".to_string(), json!(report_type));
            }
            args.insert("report_date".to_string(), json!(report_date));
            args.insert("region_code".to_string(), json!(region_code));
            if let Some(v) = max_kb {
                args.insert("max_kb".to_string(), json!(v));
            }
            if let Some(v) = max_uncompressed_kb {
                args.insert("max_uncompressed_kb".to_string(), json!(v));
            }
            if let Some(v) = max_rows {
                args.insert("max_rows".to_string(), json!(v));
            }
            if let Some(v) = max_preview_chars {
                args.insert("max_preview_chars".to_string(), json!(v));
            }
            ("download_finance_report", args)
        }
        AppStoreConnectTools::TestAuth => ("test_auth", Map::new()),
    };

    call_tool(cli, "app-store-connect", tool_name, args).await
}

/// Handle Apple Search Ads commands
pub async fn handle_apple_search_ads(cli: &Cli, tool: AppleSearchAdsTools) -> Result<()> {
    fn parse_body_json(body: &str) -> Result<Value> {
        serde_json::from_str::<Value>(body).map_err(|e| {
            crate::commands::CommandError::InvalidInput(format!(
                "Invalid JSON body: {e}. Pass a JSON string (e.g. --body '{{\"foo\":1}}')."
            ))
        })
    }

    let (tool_name, args) = match tool {
        AppleSearchAdsTools::ListCampaigns { limit, offset } => {
            let mut args = Map::new();
            if limit != 50 {
                args.insert("limit".to_string(), json!(limit));
            }
            if offset != 0 {
                args.insert("offset".to_string(), json!(offset));
            }
            ("list_campaigns", args)
        }
        AppleSearchAdsTools::KeywordRecommendations {
            app_id,
            storefront_countries,
        } => {
            let mut args = Map::new();
            args.insert("app_id".to_string(), json!(app_id));
            args.insert(
                "storefront_countries".to_string(),
                json!(storefront_countries),
            );
            ("keyword_recommendations", args)
        }
        AppleSearchAdsTools::ReportKeywords { body } => {
            let mut args = Map::new();
            args.insert("body".to_string(), parse_body_json(&body)?);
            ("report_keywords", args)
        }
        AppleSearchAdsTools::ReportSearchTerms { body } => {
            let mut args = Map::new();
            args.insert("body".to_string(), parse_body_json(&body)?);
            ("report_search_terms", args)
        }
        AppleSearchAdsTools::ReportCampaignKeywords { campaign_id, body } => {
            let mut args = Map::new();
            args.insert("campaign_id".to_string(), json!(campaign_id));
            args.insert("body".to_string(), parse_body_json(&body)?);
            ("report_campaign_keywords", args)
        }
        AppleSearchAdsTools::ReportCampaignSearchTerms { campaign_id, body } => {
            let mut args = Map::new();
            args.insert("campaign_id".to_string(), json!(campaign_id));
            args.insert("body".to_string(), parse_body_json(&body)?);
            ("report_campaign_search_terms", args)
        }
        AppleSearchAdsTools::CreateCampaign { body } => {
            let mut args = Map::new();
            args.insert("body".to_string(), parse_body_json(&body)?);
            ("create_campaign", args)
        }
        AppleSearchAdsTools::TestAuth => ("test_auth", Map::new()),
    };

    call_tool(cli, "apple-search-ads", tool_name, args).await
}

/// Handle web commands
pub async fn handle_web(cli: &Cli, tool: WebTools) -> Result<()> {
    let (tool_name, args) = match tool {
        WebTools::Scrape { url, format } => {
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            let _ = format;
            ("scrape_url", args)
        }
        WebTools::Extract { url, images, links } => {
            let _ = (images, links);
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            ("extract", args)
        }
        WebTools::Metadata { url } => {
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            ("metadata", args)
        }
    };

    match tool_name {
        "scrape_url" => call_tool(cli, "web", "scrape_url", args).await,
        "extract" => {
            let (payload, meta_value) = call_tool_raw(cli, "web", "scrape_url", args).await?;
            let extracted = payload.get("content").cloned().unwrap_or(Value::Null);
            output_tool_result(cli, "web", "extract", &extracted, meta_value.as_ref())
        }
        "metadata" => {
            let (payload, meta_value) = call_tool_raw(cli, "web", "scrape_url", args).await?;
            let extracted = payload.get("metadata").cloned().unwrap_or(Value::Null);
            output_tool_result(cli, "web", "metadata", &extracted, meta_value.as_ref())
        }
        _ => unreachable!("tool_name is constructed above"),
    }
}

/// Handle wikipedia commands
pub async fn handle_wikipedia(cli: &Cli, tool: WikipediaTools) -> Result<()> {
    let (tool_name, args) = match tool {
        WikipediaTools::Search { query, limit } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        WikipediaTools::Article { title } => {
            let mut args = Map::new();
            args.insert("title".to_string(), json!(title));
            args.insert("response_format".to_string(), json!("detailed"));
            ("get", args)
        }
        WikipediaTools::Summary { title } => {
            let mut args = Map::new();
            args.insert("title".to_string(), json!(title));
            args.insert("response_format".to_string(), json!("concise"));
            ("get", args)
        }
    };

    call_tool(cli, "wikipedia", tool_name, args).await
}

/// Handle pubmed commands
pub async fn handle_pubmed(cli: &Cli, tool: PubmedTools) -> Result<()> {
    let (tool_name, args) = match tool {
        PubmedTools::Search { query, limit } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        PubmedTools::Article { pmid } => {
            let mut args = Map::new();
            args.insert("pmid".to_string(), json!(pmid));
            args.insert("response_format".to_string(), json!("detailed"));
            ("get", args)
        }
    };

    call_tool(cli, "pubmed", tool_name, args).await
}

/// Handle semantic scholar commands
pub async fn handle_semantic_scholar(cli: &Cli, tool: SemanticScholarTools) -> Result<()> {
    let (tool_name, args) = match tool {
        SemanticScholarTools::Search { query, limit } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("page_size".to_string(), json!(limit));
            args.insert("page".to_string(), json!(1));
            ("search_papers", args)
        }
        SemanticScholarTools::Paper { id } => {
            let mut args = Map::new();
            args.insert("paper_id".to_string(), json!(id));
            ("get_paper_details", args)
        }
        SemanticScholarTools::Citations { id, limit } => {
            let mut args = Map::new();
            args.insert("paper_id".to_string(), json!(id));
            args.insert("limit".to_string(), json!(limit));
            ("get_citations", args)
        }
        SemanticScholarTools::References { id, limit } => {
            let mut args = Map::new();
            args.insert("paper_id".to_string(), json!(id));
            args.insert("limit".to_string(), json!(limit));
            ("get_references", args)
        }
    };

    call_tool(cli, "semantic-scholar", tool_name, args).await
}

/// Handle slack commands
pub async fn handle_slack(cli: &Cli, tool: SlackTools) -> Result<()> {
    let (tool_name, args) = match tool {
        SlackTools::Channels { limit, cursor } => {
            let mut args = Map::new();
            args.insert("limit".to_string(), json!(limit));
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            ("list_channels", args)
        }
        SlackTools::Messages {
            channel,
            limit,
            cursor,
        } => {
            let mut args = Map::new();
            args.insert("channel".to_string(), json!(channel));
            args.insert("limit".to_string(), json!(limit));
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            ("list_messages", args)
        }
        SlackTools::Search {
            query,
            limit,
            page,
            sort,
            sort_dir,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("count".to_string(), json!(limit));
            args.insert("page".to_string(), json!(page));
            if let Some(s) = sort {
                args.insert("sort".to_string(), json!(s));
            }
            if let Some(sd) = sort_dir {
                args.insert("sort_dir".to_string(), json!(sd));
            }
            ("search_messages", args)
        }
        SlackTools::Users { limit, cursor } => {
            let mut args = Map::new();
            args.insert("limit".to_string(), json!(limit));
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            ("list_users", args)
        }
    };

    call_tool(cli, "slack", tool_name, args).await
}

/// Handle X (Twitter) commands
pub async fn handle_x(cli: &Cli, tool: XTools) -> Result<()> {
    let (tool_name, args) = match tool {
        XTools::Profile { username } => {
            let mut args = Map::new();
            args.insert("username".to_string(), json!(username));
            ("get_profile", args)
        }
        XTools::SearchTweets {
            query,
            limit,
            cursor,
            mode,
            since,
            until,
            start_time,
            end_time,
            exclude_replies,
            exclude_retweets,
            min_likes,
            min_retweets,
            min_replies,
            min_views,
            sort_by,
            order,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            if let Some(m) = mode {
                args.insert("mode".to_string(), json!(m));
            }
            if let Some(s) = since {
                args.insert("since".to_string(), json!(s));
            }
            if let Some(u) = until {
                args.insert("until".to_string(), json!(u));
            }
            if let Some(s) = start_time {
                args.insert("start_time".to_string(), json!(s));
            }
            if let Some(e) = end_time {
                args.insert("end_time".to_string(), json!(e));
            }
            if let Some(v) = exclude_replies {
                args.insert("exclude_replies".to_string(), json!(v));
            }
            if let Some(v) = exclude_retweets {
                args.insert("exclude_retweets".to_string(), json!(v));
            }
            if let Some(v) = min_likes {
                args.insert("min_likes".to_string(), json!(v));
            }
            if let Some(v) = min_retweets {
                args.insert("min_retweets".to_string(), json!(v));
            }
            if let Some(v) = min_replies {
                args.insert("min_replies".to_string(), json!(v));
            }
            if let Some(v) = min_views {
                args.insert("min_views".to_string(), json!(v));
            }
            if let Some(v) = sort_by {
                args.insert("sort_by".to_string(), json!(v));
            }
            if let Some(v) = order {
                args.insert("order".to_string(), json!(v));
            }
            ("search_tweets", args)
        }
        XTools::Followers {
            username,
            limit,
            cursor,
        } => {
            let mut args = Map::new();
            args.insert("username".to_string(), json!(username));
            args.insert("limit".to_string(), json!(limit));
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            ("get_followers", args)
        }
        XTools::Tweet { tweet_id } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            ("get_tweet", args)
        }
        XTools::Timeline {
            count,
            exclude_replies,
        } => {
            let mut args = Map::new();
            args.insert("count".to_string(), json!(count));
            if let Some(er) = exclude_replies {
                args.insert("exclude_replies".to_string(), json!(er));
            }
            ("get_home_timeline", args)
        }
        XTools::TweetsAndReplies {
            username,
            limit,
            cursor,
        } => {
            let mut args = Map::new();
            args.insert("username".to_string(), json!(username));
            args.insert("limit".to_string(), json!(limit));
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            ("fetch_tweets_and_replies", args)
        }
        XTools::UserTweets {
            username,
            limit,
            cursor,
            exclude_retweets,
            start_time,
            end_time,
            order,
        } => {
            let mut args = Map::new();
            args.insert("username".to_string(), json!(username));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            if let Some(v) = exclude_retweets {
                args.insert("exclude_retweets".to_string(), json!(v));
            }
            if let Some(s) = start_time {
                args.insert("start_time".to_string(), json!(s));
            }
            if let Some(e) = end_time {
                args.insert("end_time".to_string(), json!(e));
            }
            if let Some(v) = order {
                args.insert("order".to_string(), json!(v));
            }
            ("get_user_tweets", args)
        }
        XTools::Thread {
            tweet_id,
            limit,
            start_time,
            end_time,
            exclude_replies,
            exclude_retweets,
            sort_by,
            order,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            if let Some(s) = start_time {
                args.insert("start_time".to_string(), json!(s));
            }
            if let Some(e) = end_time {
                args.insert("end_time".to_string(), json!(e));
            }
            if let Some(v) = exclude_replies {
                args.insert("exclude_replies".to_string(), json!(v));
            }
            if let Some(v) = exclude_retweets {
                args.insert("exclude_retweets".to_string(), json!(v));
            }
            if let Some(v) = sort_by {
                args.insert("sort_by".to_string(), json!(v));
            }
            if let Some(v) = order {
                args.insert("order".to_string(), json!(v));
            }
            ("get_thread", args)
        }
        XTools::SearchProfiles {
            query,
            limit,
            cursor,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            ("search_profiles", args)
        }
        XTools::DmConversations { user_id, cursor } => {
            let mut args = Map::new();
            args.insert("user_id".to_string(), json!(user_id));
            if let Some(c) = cursor {
                args.insert("cursor".to_string(), json!(c));
            }
            ("get_direct_message_conversations", args)
        }
        XTools::SendDm {
            conversation_id,
            text,
        } => {
            let mut args = Map::new();
            args.insert("conversation_id".to_string(), json!(conversation_id));
            args.insert("text".to_string(), json!(text));
            ("send_direct_message", args)
        }
    };

    call_tool(cli, "x-browser", tool_name, args).await
}

/// Handle X (Twitter) API commands
pub async fn handle_x_api(cli: &Cli, tool: XApiTools) -> Result<()> {
    fn insert_if_some<T: serde::Serialize>(
        args: &mut Map<String, Value>,
        key: &str,
        value: Option<T>,
    ) {
        if let Some(value) = value {
            args.insert(key.to_string(), json!(value));
        }
    }

    let (tool_name, args) = match tool {
        XApiTools::AuthStatus => ("get_auth_status", Map::new()),
        XApiTools::WhoAmI { auth_mode } => {
            let mut args = Map::new();
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("whoami", args)
        }
        XApiTools::SearchRecentTweets {
            query,
            max_results,
            pages,
            limit,
            next_token,
            since,
            start_time,
            end_time,
            sort_order,
            sort_by,
            order,
            exclude_replies,
            exclude_retweets,
            min_likes,
            min_retweets,
            min_replies,
            min_quotes,
            min_views,
            from_username,
            quick,
            quality,
            include_raw,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "pages", pages);
            insert_if_some(&mut args, "limit", limit);
            insert_if_some(&mut args, "next_token", next_token);
            insert_if_some(&mut args, "since", since);
            insert_if_some(&mut args, "start_time", start_time);
            insert_if_some(&mut args, "end_time", end_time);
            insert_if_some(&mut args, "sort_order", sort_order);
            insert_if_some(&mut args, "sort_by", sort_by);
            insert_if_some(&mut args, "order", order);
            insert_if_some(&mut args, "exclude_replies", exclude_replies);
            insert_if_some(&mut args, "exclude_retweets", exclude_retweets);
            insert_if_some(&mut args, "min_likes", min_likes);
            insert_if_some(&mut args, "min_retweets", min_retweets);
            insert_if_some(&mut args, "min_replies", min_replies);
            insert_if_some(&mut args, "min_quotes", min_quotes);
            insert_if_some(&mut args, "min_views", min_views);
            insert_if_some(&mut args, "from_username", from_username);
            insert_if_some(&mut args, "quick", quick);
            insert_if_some(&mut args, "quality", quality);
            insert_if_some(&mut args, "include_raw", include_raw);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("search_recent_tweets", args)
        }
        XApiTools::Tweet {
            tweet_id,
            tweet_fields,
            expansions,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "tweet_fields", tweet_fields);
            insert_if_some(&mut args, "expansions", expansions);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_tweet", args)
        }
        XApiTools::UserByUsername {
            username,
            user_fields,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("username".to_string(), json!(username));
            insert_if_some(&mut args, "user_fields", user_fields);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_user_by_username", args)
        }
        XApiTools::Profile {
            username,
            user_fields,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("username".to_string(), json!(username));
            insert_if_some(&mut args, "user_fields", user_fields);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_profile", args)
        }
        XApiTools::Thread {
            tweet_id,
            max_results,
            pages,
            limit,
            next_token,
            since,
            start_time,
            end_time,
            exclude_replies,
            exclude_retweets,
            order,
            include_root,
            include_raw,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "pages", pages);
            insert_if_some(&mut args, "limit", limit);
            insert_if_some(&mut args, "next_token", next_token);
            insert_if_some(&mut args, "since", since);
            insert_if_some(&mut args, "start_time", start_time);
            insert_if_some(&mut args, "end_time", end_time);
            insert_if_some(&mut args, "exclude_replies", exclude_replies);
            insert_if_some(&mut args, "exclude_retweets", exclude_retweets);
            insert_if_some(&mut args, "order", order);
            insert_if_some(&mut args, "include_root", include_root);
            insert_if_some(&mut args, "include_raw", include_raw);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_thread", args)
        }
        XApiTools::UserTweets {
            user_id,
            max_results,
            pagination_token,
            start_time,
            end_time,
            exclude_replies,
            exclude_retweets,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("user_id".to_string(), json!(user_id));
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "pagination_token", pagination_token);
            insert_if_some(&mut args, "start_time", start_time);
            insert_if_some(&mut args, "end_time", end_time);
            insert_if_some(&mut args, "exclude_replies", exclude_replies);
            insert_if_some(&mut args, "exclude_retweets", exclude_retweets);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_user_tweets", args)
        }
        XApiTools::ProfileTweets {
            username,
            max_results,
            pagination_token,
            start_time,
            end_time,
            exclude_replies,
            exclude_retweets,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("username".to_string(), json!(username));
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "pagination_token", pagination_token);
            insert_if_some(&mut args, "start_time", start_time);
            insert_if_some(&mut args, "end_time", end_time);
            insert_if_some(&mut args, "exclude_replies", exclude_replies);
            insert_if_some(&mut args, "exclude_retweets", exclude_retweets);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_user_tweets_by_username", args)
        }
        XApiTools::SearchAllTweets {
            query,
            max_results,
            next_token,
            start_time,
            end_time,
            sort_order,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "next_token", next_token);
            insert_if_some(&mut args, "start_time", start_time);
            insert_if_some(&mut args, "end_time", end_time);
            insert_if_some(&mut args, "sort_order", sort_order);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("search_all_tweets", args)
        }
        XApiTools::Mentions {
            user_id,
            max_results,
            pagination_token,
            auth_mode,
        } => {
            let mut args = Map::new();
            insert_if_some(&mut args, "user_id", user_id);
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "pagination_token", pagination_token);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_mentions", args)
        }
        XApiTools::HomeTimeline {
            max_results,
            pagination_token,
            auth_mode,
        } => {
            let mut args = Map::new();
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "pagination_token", pagination_token);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_home_timeline", args)
        }
        XApiTools::CreatePost {
            text,
            reply_to_tweet_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("text".to_string(), json!(text));
            insert_if_some(&mut args, "reply_to_tweet_id", reply_to_tweet_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("create_post", args)
        }
        XApiTools::DeletePost {
            tweet_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("delete_post", args)
        }
        XApiTools::LikePost {
            tweet_id,
            user_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "user_id", user_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("like_post", args)
        }
        XApiTools::UnlikePost {
            tweet_id,
            user_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "user_id", user_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("unlike_post", args)
        }
        XApiTools::RepostPost {
            tweet_id,
            user_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "user_id", user_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("repost_post", args)
        }
        XApiTools::UnrepostPost {
            tweet_id,
            user_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "user_id", user_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("unrepost_post", args)
        }
        XApiTools::FollowUser {
            target_user_id,
            source_user_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("target_user_id".to_string(), json!(target_user_id));
            insert_if_some(&mut args, "source_user_id", source_user_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("follow_user", args)
        }
        XApiTools::UnfollowUser {
            target_user_id,
            source_user_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("target_user_id".to_string(), json!(target_user_id));
            insert_if_some(&mut args, "source_user_id", source_user_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("unfollow_user", args)
        }
        XApiTools::GetBookmarks {
            user_id,
            max_results,
            pagination_token,
            auth_mode,
        } => {
            let mut args = Map::new();
            insert_if_some(&mut args, "user_id", user_id);
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "pagination_token", pagination_token);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_bookmarks", args)
        }
        XApiTools::AddBookmark {
            tweet_id,
            user_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "user_id", user_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("add_bookmark", args)
        }
        XApiTools::RemoveBookmark {
            tweet_id,
            user_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("tweet_id".to_string(), json!(tweet_id));
            insert_if_some(&mut args, "user_id", user_id);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("remove_bookmark", args)
        }
        XApiTools::RefreshOauth2 => ("refresh_oauth2", Map::new()),
        XApiTools::GetUsage {
            days,
            usage_fields,
            auth_mode,
        } => {
            let mut args = Map::new();
            insert_if_some(&mut args, "days", days);
            insert_if_some(&mut args, "usage_fields", usage_fields);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_usage", args)
        }
        XApiTools::CreateList {
            name,
            description,
            private,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("name".to_string(), json!(name));
            insert_if_some(&mut args, "description", description);
            insert_if_some(&mut args, "private", private);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("create_list", args)
        }
        XApiTools::UpdateList {
            list_id,
            name,
            description,
            private,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("list_id".to_string(), json!(list_id));
            insert_if_some(&mut args, "name", name);
            insert_if_some(&mut args, "description", description);
            insert_if_some(&mut args, "private", private);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("update_list", args)
        }
        XApiTools::DeleteList { list_id, auth_mode } => {
            let mut args = Map::new();
            args.insert("list_id".to_string(), json!(list_id));
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("delete_list", args)
        }
        XApiTools::CreateDmConversation {
            participant_ids,
            text,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("participant_ids".to_string(), json!(participant_ids));
            args.insert("text".to_string(), json!(text));
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("create_dm_conversation", args)
        }
        XApiTools::GetDmEvents {
            conversation_id,
            max_results,
            pagination_token,
            event_types,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("conversation_id".to_string(), json!(conversation_id));
            insert_if_some(&mut args, "max_results", max_results);
            insert_if_some(&mut args, "pagination_token", pagination_token);
            insert_if_some(&mut args, "event_types", event_types);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("get_dm_events", args)
        }
        XApiTools::InitializeMediaUpload {
            media_type,
            total_bytes,
            media_category,
            shared,
            additional_owners,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("media_type".to_string(), json!(media_type));
            args.insert("total_bytes".to_string(), json!(total_bytes));
            if !additional_owners.is_empty() {
                args.insert("additional_owners".to_string(), json!(additional_owners));
            }
            insert_if_some(&mut args, "media_category", media_category);
            insert_if_some(&mut args, "shared", shared);
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("initialize_media_upload", args)
        }
        XApiTools::AppendMediaUpload {
            upload_id,
            segment_index,
            media_base64,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("upload_id".to_string(), json!(upload_id));
            args.insert("segment_index".to_string(), json!(segment_index));
            args.insert("media_base64".to_string(), json!(media_base64));
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("append_media_upload", args)
        }
        XApiTools::FinalizeMediaUpload {
            upload_id,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("upload_id".to_string(), json!(upload_id));
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("finalize_media_upload", args)
        }
        XApiTools::RawOperation {
            operation_id,
            path_params,
            query,
            body,
            auth_mode,
        } => {
            let mut args = Map::new();
            args.insert("operation_id".to_string(), json!(operation_id));
            if let Some(raw) = path_params {
                args.insert(
                    "path_params".to_string(),
                    parse_json_argument("path_params", &raw)?,
                );
            }
            if let Some(raw) = query {
                args.insert("query".to_string(), parse_json_argument("query", &raw)?);
            }
            if let Some(raw) = body {
                args.insert("body".to_string(), parse_json_argument("body", &raw)?);
            }
            insert_if_some(&mut args, "auth_mode", auth_mode);
            ("raw_operation", args)
        }
    };

    call_tool(cli, "x", tool_name, args).await
}

/// Handle Discord commands
pub async fn handle_discord(cli: &Cli, tool: DiscordTools) -> Result<()> {
    let (tool_name, args) = match tool {
        DiscordTools::Servers => ("list_servers", Map::new()),
        DiscordTools::Server { guild_id } => {
            let mut args = Map::new();
            args.insert("guild_id".to_string(), json!(guild_id));
            ("get_server_info", args)
        }
        DiscordTools::Channels { guild_id } => {
            let mut args = Map::new();
            args.insert("guild_id".to_string(), json!(guild_id));
            ("list_channels", args)
        }
        DiscordTools::Messages { channel_id, limit } => {
            let mut args = Map::new();
            args.insert("channel_id".to_string(), json!(channel_id));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            ("read_messages", args)
        }
        DiscordTools::Send {
            channel_id,
            content,
        } => {
            let mut args = Map::new();
            args.insert("channel_id".to_string(), json!(channel_id));
            args.insert("content".to_string(), json!(content));
            ("send_message", args)
        }
        DiscordTools::Search {
            channel_id,
            query,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("channel_id".to_string(), json!(channel_id));
            args.insert("query".to_string(), json!(query));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            ("search_messages", args)
        }
    };

    call_tool(cli, "discord", tool_name, args).await
}

/// Handle RSS commands
pub async fn handle_rss(cli: &Cli, tool: RssTools) -> Result<()> {
    let (tool_name, args) = match tool {
        RssTools::Feed { url, limit } => {
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            ("get_feed", args)
        }
        RssTools::Entries { url, limit } => {
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            ("list_entries", args)
        }
        RssTools::Search { url, query, limit } => {
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            args.insert("query".to_string(), json!(query));
            if let Some(l) = limit {
                args.insert("limit".to_string(), json!(l));
            }
            ("search_feed", args)
        }
        RssTools::Discover { url } => {
            let mut args = Map::new();
            args.insert("url".to_string(), json!(url));
            ("discover_feeds", args)
        }
    };

    call_tool(cli, "rss", tool_name, args).await
}

/// Handle bioRxiv commands
pub async fn handle_biorxiv(cli: &Cli, tool: BiorxivTools) -> Result<()> {
    let (tool_name, args) = match tool {
        BiorxivTools::Recent { server, count } => {
            let mut args = Map::new();
            args.insert("server".to_string(), json!(server));
            if let Some(c) = count {
                args.insert("count".to_string(), json!(c));
            }
            ("get_recent_preprints", args)
        }
        BiorxivTools::DateRange {
            server,
            start_date,
            end_date,
        } => {
            let mut args = Map::new();
            args.insert("server".to_string(), json!(server));
            args.insert("start_date".to_string(), json!(start_date));
            args.insert("end_date".to_string(), json!(end_date));
            ("get_preprints_by_date", args)
        }
        BiorxivTools::Paper { server, doi } => {
            let mut args = Map::new();
            args.insert("server".to_string(), json!(server));
            args.insert("doi".to_string(), json!(doi));
            ("get", args)
        }
    };

    call_tool(cli, "biorxiv", tool_name, args).await
}

/// Handle scihub commands (open-access lookup)
pub async fn handle_scihub(cli: &Cli, tool: ScihubTools) -> Result<()> {
    let (tool_name, args) = match tool {
        ScihubTools::Paper { doi } => {
            let mut args = Map::new();
            args.insert("doi".to_string(), json!(doi));
            ("get", args)
        }
        ScihubTools::Search {
            query,
            limit,
            page,
            oa_only,
        } => {
            let mut args = Map::new();
            args.insert("query".to_string(), json!(query));
            args.insert("limit".to_string(), json!(limit));
            args.insert("page".to_string(), json!(page));
            args.insert("oa_only".to_string(), json!(oa_only));
            ("search", args)
        }
        ScihubTools::Batch { dois } => {
            let doi_list: Vec<&str> = dois.split(',').map(|s| s.trim()).collect();
            let mut args = Map::new();
            args.insert("dois".to_string(), json!(doi_list));
            ("batch_get", args)
        }
    };

    call_tool(cli, "scihub", tool_name, args).await
}

/// Handle Apple Messages commands
pub async fn handle_apple_messages(cli: &Cli, tool: AppleMessagesTools) -> Result<()> {
    let (tool_name, args) = match tool {
        AppleMessagesTools::Chats { limit } => {
            let mut args = Map::new();
            args.insert("limit".to_string(), json!(limit));
            ("list_chats", args)
        }
        AppleMessagesTools::Messages {
            alias,
            chat_identifier,
            since,
            since_message_id,
            limit,
        } => {
            let mut args = Map::new();
            if let Some(value) = alias {
                args.insert("alias".to_string(), json!(value));
            }
            if let Some(value) = chat_identifier {
                args.insert("chat_identifier".to_string(), json!(value));
            }
            if let Some(value) = since {
                args.insert("since".to_string(), json!(value));
            }
            if let Some(value) = since_message_id {
                args.insert("since_message_id".to_string(), json!(value));
            }
            args.insert("limit".to_string(), json!(limit));
            ("get_recent_messages", args)
        }
        AppleMessagesTools::Send {
            alias,
            recipient,
            message,
        } => {
            let mut args = Map::new();
            if let Some(value) = alias {
                args.insert("alias".to_string(), json!(value));
            }
            if let Some(value) = recipient {
                args.insert("recipient".to_string(), json!(value));
            }
            args.insert("message".to_string(), json!(message));
            ("send_message", args)
        }
        AppleMessagesTools::Aliases => ("list_aliases", Map::new()),
        AppleMessagesTools::SetAlias { alias, identifier } => {
            let mut args = Map::new();
            args.insert("alias".to_string(), json!(alias));
            args.insert("identifier".to_string(), json!(identifier));
            ("upsert_alias", args)
        }
        AppleMessagesTools::RemoveAlias { alias } => {
            let mut args = Map::new();
            args.insert("alias".to_string(), json!(alias));
            ("remove_alias", args)
        }
    };

    call_tool(cli, "apple-messages", tool_name, args).await
}

/// Handle macOS commands
pub async fn handle_macos(cli: &Cli, tool: MacosTools) -> Result<()> {
    let (tool_name, args) = match tool {
        MacosTools::Script {
            language,
            script,
            params,
            max_output_chars,
        } => {
            let mut args = Map::new();
            args.insert("language".to_string(), json!(language));
            args.insert("script".to_string(), json!(script));
            if let Some(ref p) = params {
                if let Ok(parsed) = serde_json::from_str::<Value>(p) {
                    args.insert("params".to_string(), parsed);
                }
            }
            if let Some(max) = max_output_chars {
                args.insert("max_output_chars".to_string(), json!(max));
            }
            ("run_script", args)
        }
        MacosTools::Notify {
            title,
            message,
            subtitle,
        } => {
            let mut args = Map::new();
            args.insert("message".to_string(), json!(message));
            if let Some(t) = title {
                args.insert("title".to_string(), json!(t));
            }
            if let Some(s) = subtitle {
                args.insert("subtitle".to_string(), json!(s));
            }
            ("show_notification", args)
        }
        MacosTools::Reveal { path } => {
            let mut args = Map::new();
            args.insert("path".to_string(), json!(path));
            ("reveal_in_finder", args)
        }
        MacosTools::GetClipboard => ("get_clipboard", Map::new()),
        MacosTools::SetClipboard { text } => {
            let mut args = Map::new();
            args.insert("text".to_string(), json!(text));
            ("set_clipboard", args)
        }
        MacosTools::Shortcut { name, input } => {
            let mut args = Map::new();
            args.insert("name".to_string(), json!(name));
            if let Some(ref i) = input {
                if let Ok(parsed) = serde_json::from_str::<Value>(i) {
                    args.insert("input".to_string(), parsed);
                }
            }
            ("run_shortcut", args)
        }
    };

    call_tool(cli, "macos", tool_name, args).await
}

/// Handle Spotlight commands
pub async fn handle_spotlight(cli: &Cli, tool: SpotlightTools) -> Result<()> {
    let (tool_name, args) = match tool {
        SpotlightTools::SearchContent {
            query,
            directory,
            kind,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("mode".to_string(), json!("content"));
            args.insert("query".to_string(), json!(query));
            if let Some(d) = directory {
                args.insert("directory".to_string(), json!(d));
            }
            if let Some(k) = kind {
                args.insert("kind".to_string(), json!(k));
            }
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        SpotlightTools::SearchByName {
            name,
            directory,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("mode".to_string(), json!("name"));
            args.insert("query".to_string(), json!(name));
            if let Some(d) = directory {
                args.insert("directory".to_string(), json!(d));
            }
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        SpotlightTools::SearchByKind {
            kind,
            directory,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("mode".to_string(), json!("kind"));
            args.insert("kind".to_string(), json!(kind));
            if let Some(d) = directory {
                args.insert("directory".to_string(), json!(d));
            }
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        SpotlightTools::SearchRecent {
            days,
            kind,
            directory,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("mode".to_string(), json!("recent"));
            args.insert("days".to_string(), json!(days));
            if let Some(k) = kind {
                args.insert("kind".to_string(), json!(k));
            }
            if let Some(d) = directory {
                args.insert("directory".to_string(), json!(d));
            }
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
        SpotlightTools::Metadata { path } => {
            let mut args = Map::new();
            args.insert("path".to_string(), json!(path));
            ("get_metadata", args)
        }
        SpotlightTools::RawQuery {
            query,
            directory,
            limit,
        } => {
            let mut args = Map::new();
            args.insert("mode".to_string(), json!("raw"));
            args.insert("query".to_string(), json!(query));
            if let Some(d) = directory {
                args.insert("directory".to_string(), json!(d));
            }
            args.insert("limit".to_string(), json!(limit));
            ("search", args)
        }
    };

    call_tool(cli, "spotlight", tool_name, args).await
}
