use crate::cli::Cli;
use crate::commands::report::render_tool_failure_report_block;
use crate::commands::tool_mappings::generic_search_tool_and_args;
use crate::commands::usage_helpers::print_cost_summary;
use crate::commands::{copy_to_clipboard, CommandError, Result};
use crate::output::{format_output, format_pretty, OutputData};
use indicatif::{ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use rzn_tools_core::federated::{FederatedSearch, MergeMode, ProfileStore, SearchProfile};
use rzn_tools_core::{CallToolRequestParam, ProviderRegistry};
use serde_json::{json, Value};
use std::sync::Arc;

/// Run a search command - either single connector or federated.
///
/// # Arguments
/// - `connector_or_query`: Either a connector name (single search) or the query (federated search)
/// - `query`: The search query (only used for single connector search)
/// - `limit`: Maximum results per source
/// - `profile`: Named profile for federated search (research, enterprise, social, code, web)
/// - `connectors`: Comma-separated list of connectors for ad-hoc federated search
/// - `merge`: Merge mode (grouped or interleaved)
/// - `add`: Additional connectors to add to profile
/// - `exclude`: Connectors to exclude from profile
/// - `web`: Quick flag to search web sources
#[allow(clippy::too_many_arguments)]
pub async fn run(
    cli: &Cli,
    connector_or_query: &str,
    query: Option<&str>,
    limit: u32,
    profile: Option<&str>,
    connectors: Option<&str>,
    merge: &str,
    add: Option<&str>,
    exclude: Option<&str>,
    web: bool,
) -> Result<()> {
    // Handle --web flag: use the "web" profile
    if web {
        return run_federated_search(
            cli,
            connector_or_query,
            limit,
            Some("web"),
            None,
            merge,
            add,
            exclude,
        )
        .await;
    }

    // Determine if this is a federated search or single connector search
    let is_federated = profile.is_some() || connectors.is_some();

    if is_federated {
        // Federated search: connector_or_query is the query
        run_federated_search(
            cli,
            connector_or_query,
            limit,
            profile,
            connectors,
            merge,
            add,
            exclude,
        )
        .await
    } else {
        // Single connector search: connector_or_query is the connector name
        let query = query.ok_or_else(|| {
            CommandError::InvalidInput(
                "Missing search query. Usage: rzn-tools search <connector> \"<query>\"".to_string(),
            )
        })?;
        run_single_search(cli, connector_or_query, query, limit).await
    }
}

/// Run a single connector search.
async fn run_single_search(cli: &Cli, connector_name: &str, query: &str, limit: u32) -> Result<()> {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Invalid progress template"),
    );
    spinner.set_message(format!("Searching {} for '{}'...", connector_name, query));
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let registry = create_registry(cli.auth_profile.as_deref()).await?;
    let provider = registry
        .get_provider(connector_name)
        .ok_or_else(|| CommandError::ConnectorNotFound(connector_name.to_string()))?
        .clone();

    let c = provider.lock().await;
    let (tool_name, arguments) = generic_search_tool_and_args(connector_name, query, limit)?;

    // Prepare search request
    let request = CallToolRequestParam {
        name: tool_name.into(),
        arguments: Some(arguments),
    };

    let response = match c.call_tool(request).await {
        Ok(response) => response,
        Err(err) => {
            spinner.finish_and_clear();
            let error = err.to_string();
            eprintln!();
            eprintln!(
                "{}",
                render_tool_failure_report_block(connector_name, tool_name, &error)
            );
            eprintln!();
            return Err(err.into());
        }
    };
    spinner.finish_and_clear();

    // Extract response data
    let results = if let Some(val) = &response.structured_content {
        val.clone()
    } else {
        json!({})
    };
    let meta_value = response
        .meta
        .as_ref()
        .and_then(|m| serde_json::to_value(m).ok());

    let output_data = OutputData::SearchResults {
        connector: connector_name.to_string(),
        query: query.to_string(),
        results: results.clone(),
        meta: meta_value.clone(),
    };

    match cli.output {
        crate::cli::OutputFormat::Pretty => {
            format_pretty_search_results(connector_name, query, &results)?;
        }
        _ => {
            format_output(&output_data, &cli.output)?;
        }
    }

    // Copy to clipboard if requested
    if cli.copy {
        let text = serde_json::to_string_pretty(&results)?;
        copy_to_clipboard(&text)?;
    }

    print_cost_summary(&cli.output, meta_value.as_ref());

    Ok(())
}

/// Run a federated search across multiple connectors.
#[allow(clippy::too_many_arguments)]
async fn run_federated_search(
    cli: &Cli,
    query: &str,
    limit: u32,
    profile: Option<&str>,
    connectors: Option<&str>,
    merge: &str,
    add: Option<&str>,
    exclude: Option<&str>,
) -> Result<()> {
    let merge_mode = match merge {
        "interleaved" => MergeMode::Interleaved,
        _ => MergeMode::Grouped,
    };

    // Determine sources
    let source_description = if let Some(profile_name) = profile {
        format!("profile '{}'", profile_name)
    } else if let Some(connector_list) = connectors {
        format!("connectors: {}", connector_list)
    } else {
        "default profile".to_string()
    };

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .expect("Invalid progress template"),
    );
    spinner.set_message(format!(
        "Searching {} for '{}'...",
        source_description, query
    ));
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let registry = Arc::new(create_registry(cli.auth_profile.as_deref()).await?);
    let engine = FederatedSearch::new(&registry);

    let result = if let Some(profile_name) = profile {
        // Profile-based search
        let profile_store = ProfileStore::new_default();
        let mut search_profile = profile_store.load(profile_name).ok_or_else(|| {
            CommandError::InvalidInput(format!(
                "Profile '{}' not found. Available built-in profiles: {}",
                profile_name,
                ProfileStore::list_builtin_names().join(", ")
            ))
        })?;

        // Apply limit override
        search_profile.defaults.limit = limit;

        // Apply add/exclude modifiers
        if let Some(add_connectors) = add {
            let additional: Vec<String> = add_connectors
                .split(',')
                .map(|s| s.trim().to_string())
                .collect();
            search_profile.connectors.extend(additional);
        }

        if let Some(exclude_connectors) = exclude {
            let to_exclude: Vec<&str> = exclude_connectors.split(',').map(|s| s.trim()).collect();
            search_profile
                .connectors
                .retain(|c| !to_exclude.contains(&c.as_str()));
        }

        engine
            .search_with_profile(query, &search_profile, Some(merge_mode))
            .await
    } else if let Some(connector_list) = connectors {
        // Ad-hoc connector list
        let connector_names: Vec<String> = connector_list
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();

        engine
            .search_adhoc(query, &connector_names, merge_mode)
            .await
    } else {
        // Default to research profile
        let search_profile = SearchProfile::get_builtin("research")
            .ok_or_else(|| CommandError::InvalidInput("Default profile not found".to_string()))?;

        engine
            .search_with_profile(query, &search_profile, Some(merge_mode))
            .await
    };

    spinner.finish_and_clear();

    // Convert to JSON for output
    let result_json =
        serde_json::to_value(&result).map_err(|e| CommandError::Other(e.to_string()))?;

    match cli.output {
        crate::cli::OutputFormat::Pretty => {
            format_pretty_federated_results(&result)?;
        }
        _ => {
            let output_data = OutputData::FederatedResults {
                query: query.to_string(),
                profile: profile.map(|s| s.to_string()),
                results: result_json.clone(),
            };
            format_output(&output_data, &cli.output)?;
        }
    }

    // Copy to clipboard if requested
    if cli.copy {
        let text = serde_json::to_string_pretty(&result_json)?;
        copy_to_clipboard(&text)?;
    }

    print_cost_summary(&cli.output, None);

    Ok(())
}

/// Format federated search results for pretty output.
fn format_pretty_federated_results(
    result: &rzn_tools_core::federated::FederatedSearchResult,
) -> Result<()> {
    use rzn_tools_core::federated::FederatedResults;

    // Header
    println!(
        "{} {}",
        "Federated Search:".bold().cyan(),
        result.query.yellow()
    );
    if let Some(ref profile) = result.profile {
        println!("{} {}", "Profile:".dimmed(), profile.cyan());
    }
    println!();

    let width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);

    match &result.results {
        FederatedResults::Grouped { sources } => {
            for source in sources {
                // Section header with horizontal rule
                let header = format!("{} ({} results)", source.source, source.count);
                let line_len = width.saturating_sub(header.len() + 6).min(60);
                println!(
                    "{} {} {}",
                    "──".cyan(),
                    header.green().bold(),
                    "─".repeat(line_len).cyan()
                );
                println!();

                if source.results.is_empty() {
                    println!("   {}", "No results".dimmed());
                } else {
                    for (i, r) in source.results.iter().enumerate() {
                        // Card: Number + Title
                        println!(
                            " {:>3}. {}",
                            (i + 1).to_string().cyan().bold(),
                            truncate_text(&r.title, 70).bold()
                        );

                        // URL on its own line
                        if let Some(ref url) = r.url {
                            println!("      {}", url.blue());
                        }

                        // Snippet - cleaned and truncated
                        if let Some(ref snippet) = r.snippet {
                            let clean = snippet.replace('\n', " ").replace("  ", " ");
                            let truncated = truncate_text(&clean, 100);
                            if !truncated.is_empty() {
                                println!("      {}", truncated.dimmed());
                            }
                        }

                        // Breathing room between cards
                        println!();
                    }
                }
            }
        }
        FederatedResults::Interleaved { results } => {
            let header = format!("Results ({} total, interleaved)", results.len());
            let line_len = width.saturating_sub(header.len() + 6).min(60);
            println!(
                "{} {} {}",
                "──".cyan(),
                header.green().bold(),
                "─".repeat(line_len).cyan()
            );
            println!();

            for (i, r) in results.iter().enumerate() {
                // Card: Number + Title + Source tag
                println!(
                    " {:>3}. {} {}",
                    (i + 1).to_string().cyan().bold(),
                    truncate_text(&r.title, 60).bold(),
                    format!("[{}]", r.source).dimmed()
                );

                // URL
                if let Some(ref url) = r.url {
                    println!("      {}", url.blue());
                }

                // Snippet
                if let Some(ref snippet) = r.snippet {
                    let clean = snippet.replace('\n', " ").replace("  ", " ");
                    let truncated = truncate_text(&clean, 100);
                    if !truncated.is_empty() {
                        println!("      {}", truncated.dimmed());
                    }
                }
                println!();
            }
        }
    }

    // Show errors if any
    if result.partial && !result.errors.is_empty() {
        println!();
        println!("{}", "⚠ Partial results - some sources failed:".yellow());
        for err in &result.errors {
            let timeout_marker = if err.is_timeout { " (timeout)" } else { "" };
            println!(
                "   {} {}: {}{}",
                "•".dimmed(),
                err.source.yellow(),
                err.error.dimmed(),
                timeout_marker.dimmed()
            );
        }
    }

    // Footer with timing
    println!();
    if let Some(duration) = result.duration_ms {
        println!("{}", format!("Completed in {}ms", duration).dimmed());
    }

    Ok(())
}

/// Truncate text to max length, adding ellipsis if needed
fn truncate_text(s: &str, max_len: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.chars().count() <= max_len {
        first_line.to_string()
    } else {
        let truncated: String = first_line.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}

fn format_pretty_search_results(connector: &str, query: &str, results: &Value) -> Result<()> {
    // Header
    println!("{} {}", "Search:".bold().cyan(), query.yellow());
    println!("{} {}", "Source:".dimmed(), connector.green());
    println!();

    // Use the unified pretty formatter for all connectors
    println!("{}", format_pretty(results));

    Ok(())
}

async fn create_registry(auth_profile: Option<&str>) -> Result<ProviderRegistry> {
    // Reuse the registry creation logic from list.rs
    crate::commands::list::create_registry(auth_profile).await
}
