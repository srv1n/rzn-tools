use crate::cli::Cli;
use crate::commands::{CommandError, Result};
use crate::output::{format_output, OutputData};
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, ContentArrangement, Table};
use owo_colors::OwoColorize;
use rzn_tools_core::{PaginatedRequestParam, ProviderRegistry};
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

/// Auth status for a connector
#[derive(Clone, Copy, PartialEq)]
enum AuthStatus {
    /// No authentication required
    None,
    /// Authentication required
    Required,
    /// Has auth configured (optional or configured)
    #[allow(dead_code)]
    Configured,
}

impl AuthStatus {
    fn short(&self) -> String {
        match self {
            AuthStatus::None => "".to_string(),
            AuthStatus::Required => "🔑".to_string(),
            AuthStatus::Configured => "✓".green().to_string(),
        }
    }
}

pub async fn run(cli: &Cli, connector: Option<&str>) -> Result<()> {
    let registry = crate::commands::list::create_registry(cli.auth_profile.as_deref()).await?;

    if let Some(connector_name) = connector {
        // Show tools for specific connector
        show_connector_tools(cli, &registry, connector_name).await
    } else {
        // Show all tools
        show_all_tools(cli, &registry).await
    }
}

async fn show_connector_tools(
    cli: &Cli,
    registry: &ProviderRegistry,
    connector_name: &str,
) -> Result<()> {
    let provider = registry
        .get_provider(connector_name)
        .ok_or_else(|| CommandError::ConnectorNotFound(connector_name.to_string()))?;

    let c = provider.lock().await;
    let tools_response = c
        .list_tools(Some(PaginatedRequestParam { cursor: None }))
        .await?;

    // Get auth status
    let schema = c.config_schema();
    let has_required_fields = schema.fields.iter().any(|f| f.required);
    let has_optional_fields = !schema.fields.is_empty() && !has_required_fields;

    let tools_data = json!({
        "connector": connector_name,
        "auth_required": has_required_fields,
        "auth_fields": schema.fields.iter().map(|f| json!({
            "name": f.name,
            "label": f.label,
            "required": f.required,
            "description": f.description,
        })).collect::<Vec<_>>(),
        "tools": tools_response.tools
    });

    let output_data = OutputData::ToolsList {
        connector: Some(connector_name.to_string()),
        tools: tools_data.clone(),
    };

    match cli.output {
        crate::cli::OutputFormat::Pretty => {
            format_pretty_connector_tools_with_auth(
                connector_name,
                &tools_response.tools,
                has_required_fields,
                has_optional_fields,
                &schema,
            )?;
        }
        _ => {
            format_output(&output_data, &cli.output)?;
        }
    }

    Ok(())
}

fn format_pretty_connector_tools_with_auth(
    connector_name: &str,
    tools: &[rzn_tools_core::Tool],
    has_required_fields: bool,
    has_optional_fields: bool,
    schema: &rzn_tools_core::ConnectorConfigSchema,
) -> Result<()> {
    println!("{} {}", "Tools for".bold().cyan(), connector_name.yellow());

    // Show auth status prominently
    if has_required_fields {
        println!();
        println!(
            "  {} {}",
            "🔑 Authentication:".yellow().bold(),
            "Required".yellow()
        );
        for field in &schema.fields {
            let req = if field.required { "*" } else { "" };
            println!("     {} {}{}", "•".dimmed(), field.label, req.red());
        }
        println!();
        println!(
            "  {} {}",
            "Setup:".dimmed(),
            format!("rzn-tools setup {}", connector_name).cyan()
        );
    } else if has_optional_fields {
        println!();
        println!(
            "  {} {}",
            "✓ Authentication:".green().bold(),
            "Optional".green()
        );
        for field in &schema.fields {
            println!("     {} {}", "•".dimmed(), field.label);
        }
        println!();
        println!(
            "  {} {}",
            "Setup:".dimmed(),
            format!("rzn-tools setup {}", connector_name).cyan()
        );
    } else {
        println!();
        println!(
            "  {} {}",
            "✓ Authentication:".green().bold(),
            "Not required - ready to use!".green()
        );
    }

    println!();

    // Delegate to the main formatting function
    format_pretty_connector_tools(connector_name, tools)
}

async fn show_all_tools(cli: &Cli, registry: &ProviderRegistry) -> Result<()> {
    let providers = registry.list_providers();
    let mut all_tools = Vec::new();
    let mut connector_auth: std::collections::HashMap<String, AuthStatus> =
        std::collections::HashMap::new();

    for provider_info in &providers {
        if let Some(provider) = registry.get_provider(&provider_info.name) {
            let c = provider.lock().await;

            // Determine auth status based on whether any field is required
            let schema = c.config_schema();
            let has_required_fields = schema.fields.iter().any(|f| f.required);
            let auth_status = if has_required_fields {
                AuthStatus::Required
            } else {
                AuthStatus::None
            };
            connector_auth.insert(provider_info.name.clone(), auth_status);

            if let Ok(tools_response) = c
                .list_tools(Some(PaginatedRequestParam { cursor: None }))
                .await
            {
                for tool in tools_response.tools {
                    all_tools.push(json!({
                        "connector": provider_info.name,
                        "name": tool.name,
                        "description": tool.description.as_ref().map(|d| d.to_string()).unwrap_or_else(|| "No description".to_string()),
                        "input_schema": tool.input_schema,
                        "auth_required": has_required_fields,
                    }));
                }
            }
        }
    }

    let output_data = OutputData::ToolsList {
        connector: None,
        tools: json!(all_tools),
    };

    match cli.output {
        crate::cli::OutputFormat::Pretty => {
            format_pretty_all_tools(&all_tools, &connector_auth)?;
        }
        _ => {
            format_output(&output_data, &cli.output)?;
        }
    }

    Ok(())
}

fn format_pretty_connector_tools(
    connector_name: &str,
    tools: &[rzn_tools_core::Tool],
) -> Result<()> {
    println!("{} {}", "Tools for".bold().cyan(), connector_name.yellow());
    println!();

    if tools.is_empty() {
        println!("{}", "No tools available for this connector".yellow());
        return Ok(());
    }

    let term_width = get_terminal_width() as usize;
    let desc_width = term_width.saturating_sub(40);

    // Create a table for quick overview
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(term_width as u16)
        .set_header(vec!["Tool", "Description"]);

    for tool in tools.iter() {
        let description = tool
            .description
            .as_ref()
            .map(|d| truncate_text(d, desc_width.max(30)))
            .unwrap_or_else(|| "No description".to_string());

        table.add_row(vec![tool.name.to_string(), description]);
    }

    println!("{}", table);
    println!();

    // Detailed view of each tool
    println!("{}", "Tool Details:".bold().green());
    println!();

    let separator_width = term_width.min(80);

    for (i, tool) in tools.iter().enumerate() {
        if i > 0 {
            println!();
            println!("{}", "─".repeat(separator_width).dimmed());
            println!();
        }

        println!("{}", tool.name.cyan().bold());

        if let Some(ref description) = tool.description {
            println!("  {}", description.dimmed());
        }

        // Show input schema in a readable format
        if let Ok(schema) = serde_json::from_value::<Value>(serde_json::Value::Object(
            tool.input_schema.as_ref().clone(),
        )) {
            format_tool_schema(&schema)?;
        }

        // Show how to proceed from here
        println!();
        println!("  {}", "Example:".bold());
        println!(
            "    {}",
            format!("rzn-tools {} --help", connector_name).cyan()
        );
    }

    println!();
    println!("{}", "─".repeat(separator_width).dimmed());
    println!();
    println!("{}", "Quick Commands:".bold().green());
    println!(
        "  {}",
        format!("rzn-tools search {} \"<query>\"", connector_name).cyan()
    );
    println!(
        "  {}",
        format!("rzn-tools {} --help", connector_name).cyan()
    );

    Ok(())
}

fn format_pretty_all_tools(
    tools: &[Value],
    connector_auth: &std::collections::HashMap<String, AuthStatus>,
) -> Result<()> {
    let term_width = get_terminal_width() as usize;

    println!("{}", "Available Tools".bold().cyan());
    println!();
    println!(
        "  {} = No auth needed    {} = Auth required",
        "✓".green(),
        "🔑".yellow()
    );
    println!();

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(term_width as u16)
        .set_header(vec!["Connector", "Auth", "Tool", "Description"]);

    // Calculate max description width based on terminal size
    let desc_width = term_width.saturating_sub(60); // Reserve space for other columns

    for tool in tools {
        let connector = tool
            .get("connector")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let auth_status = connector_auth
            .get(connector)
            .copied()
            .unwrap_or(AuthStatus::None);

        let auth_icon = auth_status.short();

        let name = tool
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let description = tool
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("No description");

        let description_display = truncate_text(description, desc_width.max(30));

        // Don't apply colors to table cells - let comfy-table handle widths correctly
        table.add_row(vec![
            connector.to_string(),
            auth_icon,
            name.to_string(),
            description_display,
        ]);
    }

    println!("{}", table);
    println!();

    // Group by connector with auth status
    println!("{}", "By Connector:".bold().green());

    let mut connectors: std::collections::HashMap<String, (Vec<String>, AuthStatus)> =
        std::collections::HashMap::new();

    for tool in tools {
        if let (Some(connector), Some(name)) = (
            tool.get("connector").and_then(|v| v.as_str()),
            tool.get("name").and_then(|v| v.as_str()),
        ) {
            let auth_status = connector_auth
                .get(connector)
                .copied()
                .unwrap_or(AuthStatus::None);
            connectors
                .entry(connector.to_string())
                .or_insert_with(|| (Vec::new(), auth_status))
                .0
                .push(name.to_string());
        }
    }

    // Sort by connector name
    let mut sorted: Vec<_> = connectors.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    for (connector, (tool_names, auth_status)) in sorted {
        let auth_indicator = if auth_status == AuthStatus::Required {
            " 🔑".to_string()
        } else {
            "".to_string()
        };
        println!(
            "  {}{}: {} tools",
            connector.cyan().bold(),
            auth_indicator,
            tool_names.len().to_string().green()
        );
    }

    println!();
    println!(
        "{} Use {} to see details for a connector",
        "Tip:".dimmed(),
        "rzn-tools tools <connector>".cyan()
    );

    Ok(())
}

fn format_tool_schema(schema: &Value) -> Result<()> {
    if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
        if !properties.is_empty() {
            println!("{}", "Parameters:".bold());

            let required = schema
                .get("required")
                .and_then(|r| r.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<std::collections::HashSet<_>>()
                })
                .unwrap_or_default();

            for (param_name, param_schema) in properties {
                let is_required = required.contains(param_name.as_str());
                let requirement = if is_required {
                    " (required)".red().to_string()
                } else {
                    " (optional)".dimmed().to_string()
                };

                let param_type = param_schema
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown");

                println!(
                    "  {} {}{}",
                    param_name.cyan(),
                    format!("[{}]", param_type).dimmed(),
                    requirement
                );

                if let Some(description) = param_schema.get("description").and_then(|d| d.as_str())
                {
                    println!("    {}", description.dimmed());
                }

                if let Some(default) = param_schema.get("default") {
                    println!(
                        "    {} {}",
                        "Default:".dimmed(),
                        default.to_string().dimmed()
                    );
                }
            }
        }
    }

    Ok(())
}
