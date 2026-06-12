use crate::cli::Cli;
use crate::commands::Result;
use crate::output::{format_output, OutputData};
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, ContentArrangement, Table};
use owo_colors::OwoColorize;
use rzn_tools_core::auth_store::{AuthStore, FileAuthStore};
use rzn_tools_core::{ProviderRegistry, UsageManager};

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
    let registry = create_registry(cli.auth_profile.as_deref()).await?;
    let providers = registry.list_providers();

    if providers.is_empty() {
        println!("{}", "No connectors available in this build.".yellow());
        println!();
        println!(
            "{} If you built from source, enable connector features (or use {}):",
            "Tip:".green().bold(),
            "--features full".cyan()
        );
        println!(
            "  {}",
            "cargo build --release -p rzn_tools_cli --features default-connectors".cyan()
        );
        println!(
            "  {}",
            "cargo build --release -p rzn_tools_cli --features full".cyan()
        );
        return Ok(());
    }

    let output_data = OutputData::ConnectorList(providers.clone());

    match cli.output {
        crate::cli::OutputFormat::Pretty => {
            let term_width = get_terminal_width() as usize;
            let desc_width = term_width.saturating_sub(30);

            println!("{}", "Available Data Sources".bold().cyan());
            println!();

            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .set_content_arrangement(ContentArrangement::Dynamic)
                .set_width(term_width as u16)
                .set_header(vec!["Name", "Description"]);

            for provider in &providers {
                table.add_row(vec![
                    provider.name.clone(),
                    truncate_text(&provider.description, desc_width.max(30)),
                ]);
            }

            println!("{}", table);
            println!();
            println!(
                "{} Use {} to see available tools for a connector",
                "Tip:".green().bold(),
                "rzn-tools tools <connector>".cyan()
            );
        }
        _ => {
            format_output(&output_data, &cli.output)?;
        }
    }

    Ok(())
}

pub async fn create_registry(auth_profile: Option<&str>) -> Result<ProviderRegistry> {
    // Use the core helper to build a registry with only feature-enabled connectors.
    let registry = match UsageManager::new_default() {
        Ok(usage) => {
            rzn_tools_core::build_registry_enabled_only_with_usage(std::sync::Arc::new(usage)).await
        }
        Err(err) => {
            tracing::debug!(
                "Usage manager init failed, continuing without metering: {}",
                err
            );
            rzn_tools_core::build_registry_enabled_only().await
        }
    };

    // Load saved credentials from auth store and set them on each connector
    let auth_store = FileAuthStore::new_default();
    for provider_info in registry.list_providers() {
        let provider_candidates: &[&str] = match provider_info.name.as_str() {
            // Back-compat: older versions stored browser-cookie auth under "x".
            "x-browser" => &["x-browser", "x"],
            // Back-compat: older versions stored API bearer auth under "x-api".
            "x" => &["x", "x-api"],
            _ => &[&provider_info.name],
        };

        let profile = match auth_profile {
            Some(p) => Some(p.to_string()),
            None => provider_candidates
                .iter()
                .find_map(|name| auth_store.resolve_profile_for_provider(name)),
        };
        let Some(profile) = profile else {
            continue;
        };

        let auth = provider_candidates.iter().find_map(|name| {
            let key = FileAuthStore::key_for_profile(name, &profile);
            auth_store.load(&key)
        });
        let Some(auth) = auth else {
            continue;
        };

        if let Some(provider) = registry.get_provider(&provider_info.name) {
            let mut connector = provider.lock().await;
            // Silently set auth - errors are ok (connector might not need this auth)
            let _ = connector.set_auth_details(auth).await;
        }
    }

    Ok(registry)
}
